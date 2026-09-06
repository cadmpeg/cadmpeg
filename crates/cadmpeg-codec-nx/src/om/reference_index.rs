// SPDX-License-Identifier: Apache-2.0
//! Required operation-reference indices with their exact token encoding.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Direct(u8),
    Compact([u8; 2]),
    Word([u8; 3]),
    PayloadByte([u8; 2]),
    PayloadWord([u8; 3]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "ReferenceIndexWire", into = "ReferenceIndexWire")]
pub(crate) struct ReferenceIndexToken(Encoding);

impl ReferenceIndexToken {
    pub(crate) fn read_payload(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0xf0, value, ..] => Some(Self(Encoding::PayloadByte([0xf0, *value]))),
            [0xf1, high, low, ..] if *high != 0 => {
                Some(Self(Encoding::PayloadWord([0xf1, *high, *low])))
            }
            _ => None,
        }
    }

    pub(crate) fn read_feature(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [value @ 0..=0x7f, ..] => Some(Self(Encoding::Direct(*value))),
            [high @ 0x80..=0x8f, low, ..] => Some(Self(Encoding::Compact([*high, *low]))),
            [0x90, high, low, ..] => Some(Self(Encoding::Word([0x90, *high, *low]))),
            _ => None,
        }
    }

    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let token = Self::read_payload(raw).or_else(|| Self::read_feature(raw))
            .filter(|token| token.raw().len() == raw.len())
            .ok_or("raw_object_index: invalid required reference token")?;
        if token.value() != value {
            return Err("object_index disagrees with raw_object_index");
        }
        Ok(token)
    }

    pub(crate) fn value(self) -> u32 {
        match self.0 {
            Encoding::Direct(value) | Encoding::PayloadByte([_, value]) => u32::from(value),
            Encoding::Compact([high, low]) => (u32::from(high - 0x80) << 8) | u32::from(low),
            Encoding::Word([_, high, low]) | Encoding::PayloadWord([_, high, low]) => {
                (u32::from(high) << 8) | u32::from(low)
            }
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match &self.0 {
            Encoding::Direct(value) => std::slice::from_ref(value),
            Encoding::Compact(raw) | Encoding::PayloadByte(raw) => raw,
            Encoding::Word(raw) | Encoding::PayloadWord(raw) => raw,
        }
    }
}

/// Required index restricted to the direct/compact/word feature grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FeatureReferenceToken(ReferenceIndexToken);

impl FeatureReferenceToken {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        ReferenceIndexToken::read_feature(bytes).map(Self)
    }

    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let token = Self::read(raw).ok_or("invalid feature reference token")?;
        if token.raw().len() != raw.len() || token.value() != value {
            return Err("feature index/raw token: value or width mismatch");
        }
        Ok(token)
    }

    pub(crate) fn value(self) -> u32 { self.0.value() }
    pub(crate) fn raw(&self) -> &[u8] { self.0.raw() }
}

/// Required index restricted to the payload `f0`/`f1` grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayloadIndexToken(ReferenceIndexToken);

impl PayloadIndexToken {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        ReferenceIndexToken::read_payload(bytes).map(Self)
    }

    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let token = Self::read(raw).ok_or("invalid payload reference token")?;
        if token.raw().len() != raw.len() || token.value() != value {
            return Err("payload index/raw token: value or width mismatch");
        }
        Ok(token)
    }

    pub(crate) fn value(self) -> u32 { self.0.value() }
    pub(crate) fn raw(&self) -> &[u8] { self.0.raw() }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReferenceIndexWire {
    object_index: u32,
    raw_object_index: Vec<u8>,
}

impl From<ReferenceIndexToken> for ReferenceIndexWire {
    fn from(token: ReferenceIndexToken) -> Self {
        Self { object_index: token.value(), raw_object_index: token.raw().to_vec() }
    }
}

impl TryFrom<ReferenceIndexWire> for ReferenceIndexToken {
    type Error = &'static str;

    fn try_from(wire: ReferenceIndexWire) -> Result<Self, Self::Error> {
        Self::from_wire(wire.object_index, &wire.raw_object_index)
    }
}

#[cfg(test)]
mod tests {
    use super::ReferenceIndexToken;

    #[test]
    fn reference_grammars_preserve_marker_and_width() {
        for (raw, value) in [(&[0xf0, 0][..], 0), (&[0xf0, 255][..], 255), (&[0xf1, 1, 0][..], 256), (&[0xf1, 255, 255][..], 65535)] {
            let token = ReferenceIndexToken::read_payload(raw).unwrap();
            assert_eq!(token.value(), value);
            assert_eq!(token.raw(), raw);
            assert!(ReferenceIndexToken::read_feature(raw).is_none());
        }
        for raw in [&[0][..], &[0x80, 0][..], &[0x90, 0, 0][..]] {
            let token = ReferenceIndexToken::read_feature(raw).unwrap();
            assert_eq!(token.value(), 0);
            assert_eq!(token.raw(), raw);
            assert!(ReferenceIndexToken::read_payload(raw).is_none());
        }
        for raw in [&[][..], &[0xff][..], &[0xf0][..], &[0xf1, 0, 255][..], &[0xf1, 1][..]] {
            assert!(ReferenceIndexToken::read_payload(raw).is_none());
        }
        for raw in [&[][..], &[0xff][..], &[0x80][..], &[0x90, 0][..], &[0x91, 0, 0][..], &[0xa0, 0, 0][..]] {
            assert!(ReferenceIndexToken::read_feature(raw).is_none());
        }
    }
}
