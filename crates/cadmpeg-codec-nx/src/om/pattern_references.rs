// SPDX-License-Identifier: Apache-2.0
//! Pattern reference framing, fixed slots, and derived token positions.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use super::PayloadObjectReference;

/// Byte layout selected by a pattern construction-reference field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PatternPayloadReferenceLayout {
    /// The `61`/`ff 00 ff 01`/`ff 62` graph framing.
    CanonicalGraph,
    /// The `3b`/`ff 00 01`/`ff 3c` graph framing.
    CompactGraph,
    /// The one-reference `Geometry Instance` framing.
    GeometryInstance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PatternReferences {
    offset: usize,
    body: Body,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Body {
    Graph {
        framing: GraphFraming,
        required: [PayloadIndexToken; 9],
        terminal: Option<PayloadIndexToken>,
    },
    Instance(PayloadIndexToken),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphFraming {
    Canonical,
    Compact,
}

impl GraphFraming {
    fn marker(self) -> u8 {
        match self {
            Self::Canonical => 0x61,
            Self::Compact => 0x3b,
        }
    }
    fn separator(self) -> &'static [u8] {
        match self {
            Self::Canonical => &[0xff, 0, 0xff, 1],
            Self::Compact => &[0xff, 0, 1],
        }
    }
    fn middle(self) -> &'static [u8] {
        match self {
            Self::Canonical => &[0xff, 0x62],
            Self::Compact => &[0xff, 0x3c],
        }
    }
}

const TAIL_PREFIX: [u8; 4] = [0xff, 0, 0, 1];
const SUFFIX: [u8; 3] = [0xff, 0xff, 1];
const INSTANCE_PREFIX: [u8; 3] = [0, 0xff, 0xff];
const INSTANCE_SUFFIX: [u8; 17] = [
    1, 2, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 1, 2,
];

impl PatternReferences {
    pub(crate) fn read(record: OperationPayload<'_>) -> Option<Self> {
        let bytes = record.payload();
        let consume = |at: &mut usize, expected: &[u8]| {
            let end = at.checked_add(expected.len())?;
            if bytes.get(*at..end) != Some(expected) {
                return None;
            }
            *at = end;
            Some(())
        };
        let read_token = |at: &mut usize| {
            let token = PayloadIndexToken::read(bytes.get(*at..)?)?;
            *at += token.raw().len();
            Some(token)
        };
        let decode = |start: usize| {
            let mut at = start;
            let body = match record.name() {
                "Pattern Feature" | "Pattern Geometry" => {
                    let framing = match bytes.get(at)? {
                        0x61 => GraphFraming::Canonical,
                        0x3b => GraphFraming::Compact,
                        _ => return None,
                    };
                    at += 1;
                    let first = read_token(&mut at)?;
                    consume(&mut at, framing.separator())?;
                    let second = read_token(&mut at)?;
                    let third = read_token(&mut at)?;
                    consume(&mut at, &[framing.marker()])?;
                    let fourth = read_token(&mut at)?;
                    consume(&mut at, framing.separator())?;
                    let fifth = read_token(&mut at)?;
                    let sixth = read_token(&mut at)?;
                    consume(&mut at, framing.middle())?;
                    let seventh = read_token(&mut at)?;
                    let eighth = read_token(&mut at)?;
                    consume(&mut at, &TAIL_PREFIX)?;
                    let ninth = read_token(&mut at)?;
                    let terminal = if bytes.get(at) == Some(&0xff) {
                        at += 1;
                        None
                    } else {
                        Some(read_token(&mut at)?)
                    };
                    consume(&mut at, &SUFFIX)?;
                    Body::Graph {
                        framing,
                        required: [
                            first, second, third, fourth, fifth, sixth, seventh, eighth, ninth,
                        ],
                        terminal,
                    }
                }
                "Geometry Instance" => {
                    consume(&mut at, &INSTANCE_PREFIX)?;
                    let reference = read_token(&mut at)?;
                    consume(&mut at, &INSTANCE_SUFFIX)?;
                    Body::Instance(reference)
                }
                _ => return None,
            };
            Some(Self {
                offset: record.payload_offset() + start,
                body,
            })
        };
        super::unique_candidate((0..bytes.len()).filter_map(decode))
    }

    pub(crate) fn layout(&self) -> PatternPayloadReferenceLayout {
        match &self.body {
            Body::Graph {
                framing: GraphFraming::Canonical,
                ..
            } => PatternPayloadReferenceLayout::CanonicalGraph,
            Body::Graph {
                framing: GraphFraming::Compact,
                ..
            } => PatternPayloadReferenceLayout::CompactGraph,
            Body::Instance(_) => PatternPayloadReferenceLayout::GeometryInstance,
        }
    }

    pub(crate) fn into_references(self) -> Vec<PayloadObjectReference<PayloadIndexToken>> {
        match self.body {
            Body::Instance(token) => vec![PayloadObjectReference {
                offset: self.offset + INSTANCE_PREFIX.len(),
                token,
            }],
            Body::Graph {
                framing,
                required,
                terminal,
            } => {
                let mut at = self.offset + 1;
                required
                    .into_iter()
                    .chain(terminal)
                    .enumerate()
                    .map(|(ordinal, token)| {
                        at += match ordinal {
                            1 | 4 => framing.separator().len(),
                            3 => 1,
                            6 => framing.middle().len(),
                            8 => TAIL_PREFIX.len(),
                            _ => 0,
                        };
                        let offset = at;
                        at += token.raw().len();
                        PayloadObjectReference { offset, token }
                    })
                    .collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationPayload, PatternReferences};

    #[test]
    fn mixed_width_graph_references_keep_framed_positions() -> Result<(), Box<dyn std::error::Error>>
    {
        let bytes = b"\x61\xf0\x01\xff\x00\xff\x01\xf1\x01\x02\xf0\x03\x61\xf1\x01\x04\xff\x00\xff\x01\xf0\x05\xf1\x01\x06\xff\x62\xf0\x07\xf1\x01\x08\xff\x00\x00\x01\xf0\x09\xf1\x01\x0a\xff\xff\x01";
        let payload = OperationPayload::new(bytes, 100, "Pattern Geometry")
            .ok_or("bounded operation payload")?;
        let graph = PatternReferences::read(payload).ok_or("complete pattern graph")?;
        assert_eq!(
            graph
                .into_references()
                .into_iter()
                .map(|reference| (reference.token.value(), reference.offset))
                .collect::<Vec<_>>(),
            [
                (1, 101),
                (258, 107),
                (3, 110),
                (260, 113),
                (5, 120),
                (262, 122),
                (7, 127),
                (264, 129),
                (9, 136),
                (266, 138)
            ]
        );
        Ok(())
    }
}
