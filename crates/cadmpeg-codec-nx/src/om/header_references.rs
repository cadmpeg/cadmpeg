// SPDX-License-Identifier: Apache-2.0
//! Four nullable feature references in operation-header order.

use super::reference_index::FeatureReferenceToken;

// Five marker bytes, eight scalar bytes, and two ff bytes precede the slots.
const FIXED_HEADER_LEN: usize = 5 + 8 + 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "HeaderReferencesWire", into = "HeaderReferencesWire")]
pub(crate) struct HeaderReferences(pub(crate) [Option<FeatureReferenceToken>; 4]);

impl HeaderReferences {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let mut at = 0;
        let mut tokens = [None; 4];
        for token in &mut tokens {
            if *bytes.get(at)? == 0xff {
                at += 1;
            } else {
                let value = FeatureReferenceToken::read(bytes.get(at..)?)?;
                at += value.raw().len();
                *token = Some(value);
            }
        }
        Some(Self(tokens))
    }

    fn byte_len(self) -> usize {
        self.0.iter().map(|token| token.as_ref().map_or(1, |token| token.raw().len())).sum()
    }

    pub(crate) fn values(self) -> [Option<u32>; 4] {
        self.0.map(|token| token.map(FeatureReferenceToken::value))
    }

    pub(crate) fn from_wire(values: [Option<u32>; 4], raw: [&[u8]; 4]) -> Result<Self, String> {
        let mut tokens = [None; 4];
        for (slot, (value, raw)) in values.into_iter().zip(raw).enumerate() {
            tokens[slot] = match value {
                None if raw == [0xff] => None,
                None => return Err(format!("raw_object_indices[{slot}]: null requires ff")),
                Some(value) => Some(FeatureReferenceToken::from_wire(value, raw)
                    .map_err(|error| format!("object_indices/raw_object_indices[{slot}]: {error}"))?),
            };
        }
        Ok(Self(tokens))
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct HeaderReferencesWire {
    object_indices: [Option<u32>; 4],
    raw_object_indices: [Vec<u8>; 4],
}

impl From<HeaderReferences> for HeaderReferencesWire {
    fn from(value: HeaderReferences) -> Self {
        Self {
            object_indices: value.values(),
            raw_object_indices: value.0.map(|token| token.map_or_else(|| vec![0xff], |token| token.raw().to_vec())),
        }
    }
}

impl TryFrom<HeaderReferencesWire> for HeaderReferences {
    type Error = String;

    fn try_from(wire: HeaderReferencesWire) -> Result<Self, Self::Error> {
        Self::from_wire(wire.object_indices, wire.raw_object_indices.each_ref().map(Vec::as_slice))
    }
}

/// Complete header location with all positions derived from its token widths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationHeader {
    offset: usize,
    objects: HeaderReferences,
}

impl OperationHeader {
    pub(crate) fn new(offset: usize, objects: HeaderReferences) -> Option<Self> {
        offset.checked_add(FIXED_HEADER_LEN + objects.byte_len())?;
        Some(Self { offset, objects })
    }

    pub(crate) fn offset(self) -> usize { self.offset }
    pub(crate) fn objects(self) -> HeaderReferences { self.objects }
    pub(crate) fn end_offset(self) -> usize { self.offset + FIXED_HEADER_LEN + self.objects.byte_len() }
    pub(crate) fn object_offsets(self) -> [usize; 4] {
        let mut at = self.offset + FIXED_HEADER_LEN;
        self.objects.0.map(|token| {
            let offset = at;
            at += token.as_ref().map_or(1, |token| token.raw().len());
            offset
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{HeaderReferences, OperationHeader};

    #[test]
    fn header_reference_widths_determine_every_position() {
        let objects = HeaderReferences::read(&[0xff, 0, 0x80, 0, 0x90, 0, 0, 0x03]).unwrap();
        assert_eq!(objects.values(), [None, Some(0), Some(0), Some(0)]);
        assert_eq!(objects.0.map(|token| token.map(|token| token.raw().to_vec())),
            [None, Some(vec![0]), Some(vec![0x80, 0]), Some(vec![0x90, 0, 0])]);
        let header = OperationHeader::new(100, objects).unwrap();
        assert_eq!(header.offset(), 100);
        assert_eq!(header.object_offsets(), [115, 116, 117, 119]);
        assert_eq!(header.end_offset(), 122);
        assert_eq!(OperationHeader::new(usize::MAX - 22, objects).unwrap().end_offset(), usize::MAX);
        assert!(OperationHeader::new(usize::MAX - 21, objects).is_none());
    }

    #[test]
    fn header_requires_four_complete_feature_tokens() {
        for bytes in [&[][..], &[0xff, 0xff, 0xff], &[0xff, 0xff, 0xff, 0x80],
            &[0xff, 0xff, 0xff, 0x90, 0], &[0xff, 0xff, 0xff, 0xf0, 0],
            &[0xff, 0xff, 0xff, 0xf1, 1, 0]] {
            assert!(HeaderReferences::read(bytes).is_none());
        }
        assert_eq!(HeaderReferences::read(&[0xff; 4]).unwrap().values(), [None; 4]);
    }
}
