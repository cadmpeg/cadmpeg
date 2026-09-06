// SPDX-License-Identifier: Apache-2.0
//! Four nullable feature references in operation-header order.

use super::reference_index::FeatureReferenceToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "HeaderReferencesWire", into = "HeaderReferencesWire")]
pub(crate) struct HeaderReferences(pub(crate) [Option<FeatureReferenceToken>; 4]);

impl HeaderReferences {
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
