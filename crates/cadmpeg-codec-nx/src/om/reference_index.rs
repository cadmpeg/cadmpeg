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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "ReferenceIndexWire")]
pub(crate) struct ReferenceIndexToken(Encoding);

impl ReferenceIndexToken {
    pub(super) fn read_payload(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0xf0, value, ..] => Some(Self(Encoding::PayloadByte([0xf0, *value]))),
            [0xf1, high, low, ..] if *high != 0 => {
                Some(Self(Encoding::PayloadWord([0xf1, *high, *low])))
            }
            _ => None,
        }
    }

    pub(super) fn read_feature(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [value @ 0..=0x7f, ..] => Some(Self(Encoding::Direct(*value))),
            [high @ 0x80..=0x8f, low, ..] => Some(Self(Encoding::Compact([*high, *low]))),
            [0x90, high, low, ..] => Some(Self(Encoding::Word([0x90, *high, *low]))),
            _ => None,
        }
    }

    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let token = Self::read_payload(raw)
            .or_else(|| Self::read_feature(raw))
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "ReferenceIndexWire")]
pub(crate) struct FeatureReferenceToken(ReferenceIndexToken);

impl FeatureReferenceToken {
    pub(super) fn read(bytes: &[u8]) -> Option<Self> {
        ReferenceIndexToken::read_feature(bytes).map(Self)
    }

    pub(crate) fn with_width(value: u32, width: u64) -> Option<Self> {
        let encoding = match (value, width) {
            (0..=0x7f, 1) => Encoding::Direct(value as u8),
            (0..=0xfff, 2) => Encoding::Compact([0x80 | (value >> 8) as u8, value as u8]),
            (0..=0xffff, 3) => Encoding::Word([0x90, (value >> 8) as u8, value as u8]),
            _ => return None,
        };
        Some(Self(ReferenceIndexToken(encoding)))
    }

    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let token = Self::read(raw).ok_or("raw_object_index: invalid feature reference token")?;
        if token.raw().len() != raw.len() || token.value() != value {
            return Err("object_index/raw_object_index: value or width mismatch");
        }
        Ok(token)
    }

    pub(crate) fn value(self) -> u32 {
        self.0.value()
    }
    pub(crate) fn raw(&self) -> &[u8] {
        self.0.raw()
    }
}

#[cfg(test)]
impl From<FeatureReferenceToken> for ReferenceIndexWire {
    fn from(token: FeatureReferenceToken) -> Self {
        TOKEN_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            object_index: token.value(),
            raw_object_index: token.raw().to_vec(),
        }
    }
}

impl TryFrom<ReferenceIndexWire> for FeatureReferenceToken {
    type Error = &'static str;

    fn try_from(wire: ReferenceIndexWire) -> Result<Self, Self::Error> {
        Self::from_wire(wire.object_index, &wire.raw_object_index)
    }
}

/// Feature reference encoded with the shortest permitted token width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CanonicalFeatureReferenceToken(FeatureReferenceToken);

impl CanonicalFeatureReferenceToken {
    pub(super) fn read(bytes: &[u8]) -> Option<Self> {
        let token = FeatureReferenceToken::read(bytes)?;
        let width = match token.value() {
            0..=0x7f => 1,
            0x80..=0xfff => 2,
            _ => 3,
        };
        (token.raw().len() == width).then_some(Self(token))
    }

    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let token = Self::read(raw).ok_or("invalid canonical feature reference token")?;
        if token.raw().len() != raw.len() || token.value() != value {
            return Err("object_index/raw_object_index: value or width mismatch");
        }
        Ok(token)
    }

    pub(crate) fn value(self) -> u32 {
        self.0.value()
    }
    pub(crate) fn raw(&self) -> &[u8] {
        self.0.raw()
    }
}

/// Required index restricted to the payload `f0`/`f1` grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "ReferenceIndexWire")]
pub(crate) struct PayloadIndexToken(ReferenceIndexToken);

impl PayloadIndexToken {
    pub(super) fn read(bytes: &[u8]) -> Option<Self> {
        ReferenceIndexToken::read_payload(bytes).map(Self)
    }

    pub(crate) fn from_wire(value: u32, raw: &[u8]) -> Result<Self, &'static str> {
        let token = Self::read(raw).ok_or("raw_object_index: invalid payload reference token")?;
        if token.raw().len() != raw.len() || token.value() != value {
            return Err("object_index/raw_object_index: value or width mismatch");
        }
        Ok(token)
    }

    pub(crate) fn value(self) -> u32 {
        self.0.value()
    }
    pub(crate) fn raw(&self) -> &[u8] {
        self.0.raw()
    }
}

#[derive(serde::Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct ReferenceIndexWire {
    object_index: u32,
    raw_object_index: Vec<u8>,
}

#[derive(serde::Serialize)]
struct ReferenceIndexRef<'a> {
    object_index: u32,
    raw_object_index: &'a [u8],
}

impl serde::Serialize for ReferenceIndexToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ReferenceIndexRef {
            object_index: self.value(),
            raw_object_index: self.raw(),
        }
        .serialize(serializer)
    }
}

impl serde::Serialize for FeatureReferenceToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ReferenceIndexRef {
            object_index: self.value(),
            raw_object_index: self.raw(),
        }
        .serialize(serializer)
    }
}

impl serde::Serialize for PayloadIndexToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ReferenceIndexRef {
            object_index: self.value(),
            raw_object_index: self.raw(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
std::thread_local! {
    static TOKEN_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<ReferenceIndexToken> for ReferenceIndexWire {
    fn from(token: ReferenceIndexToken) -> Self {
        TOKEN_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            object_index: token.value(),
            raw_object_index: token.raw().to_vec(),
        }
    }
}

impl TryFrom<ReferenceIndexWire> for ReferenceIndexToken {
    type Error = &'static str;

    fn try_from(wire: ReferenceIndexWire) -> Result<Self, Self::Error> {
        Self::from_wire(wire.object_index, &wire.raw_object_index)
    }
}

#[cfg(test)]
impl From<PayloadIndexToken> for ReferenceIndexWire {
    fn from(token: PayloadIndexToken) -> Self {
        TOKEN_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            object_index: token.value(),
            raw_object_index: token.raw().to_vec(),
        }
    }
}

impl TryFrom<ReferenceIndexWire> for PayloadIndexToken {
    type Error = &'static str;
    fn try_from(wire: ReferenceIndexWire) -> Result<Self, Self::Error> {
        Self::from_wire(wire.object_index, &wire.raw_object_index)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FeatureReferenceToken, PayloadIndexToken, ReferenceIndexToken, ReferenceIndexWire,
        TOKEN_INTO_WIRE_COUNT,
    };

    fn assert_borrowed_token<T: serde::Serialize>(
        token: &T,
        old_wire: ReferenceIndexWire,
        id: &'static str,
    ) {
        #[derive(serde::Serialize)]
        struct Record<'a, T> {
            id: &'static str,
            value: &'a T,
        }

        assert_eq!(
            serde_json::to_vec(token).unwrap(),
            serde_json::to_vec(&old_wire).unwrap()
        );
        let expected = serde_json::json!({"id": id, "value": old_wire});
        TOKEN_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &Record { id, value: token },
            expected,
        );
        TOKEN_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn reference_index_token_borrowed_wire_refuses_before_vec_conversion() {
        for (value, raw) in [
            (0, &[0][..]),
            (255, &[0xf0, 255][..]),
            (256, &[0xf1, 1, 0][..]),
        ] {
            let token = ReferenceIndexToken::from_wire(value, raw).unwrap();
            assert_borrowed_token(
                &token,
                ReferenceIndexWire::from(token),
                "nx:test:reference-index#1",
            );
        }
    }

    #[test]
    fn feature_reference_token_borrowed_wire_refuses_before_vec_conversion() {
        for (value, raw) in [(0, &[0][..]), (0, &[0x80, 0][..]), (256, &[0x90, 1, 0][..])] {
            let token = FeatureReferenceToken::from_wire(value, raw).unwrap();
            assert_borrowed_token(
                &token,
                ReferenceIndexWire::from(token),
                "nx:test:feature-reference#1",
            );
        }
    }

    #[test]
    fn payload_reference_token_borrowed_wire_refuses_before_vec_conversion() {
        for (value, raw) in [
            (0, &[0xf0, 0][..]),
            (255, &[0xf0, 255][..]),
            (256, &[0xf1, 1, 0][..]),
        ] {
            let token = PayloadIndexToken::from_wire(value, raw).unwrap();
            assert_borrowed_token(
                &token,
                ReferenceIndexWire::from(token),
                "nx:test:payload-reference#1",
            );
        }
    }

    #[test]
    fn reference_grammars_preserve_marker_and_width() {
        for (raw, value) in [
            (&[0xf0, 0][..], 0),
            (&[0xf0, 255][..], 255),
            (&[0xf1, 1, 0][..], 256),
            (&[0xf1, 255, 255][..], 65535),
        ] {
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
        for raw in [
            &[][..],
            &[0xff][..],
            &[0xf0][..],
            &[0xf1, 0, 255][..],
            &[0xf1, 1][..],
        ] {
            assert!(ReferenceIndexToken::read_payload(raw).is_none());
        }
        for raw in [
            &[][..],
            &[0xff][..],
            &[0x80][..],
            &[0x90, 0][..],
            &[0x91, 0, 0][..],
            &[0xa0, 0, 0][..],
        ] {
            assert!(ReferenceIndexToken::read_feature(raw).is_none());
        }
    }

    #[test]
    fn feature_reference_width_constructor_preserves_alternate_encodings() {
        use super::FeatureReferenceToken;

        for (value, width, raw) in [
            (0, 1, &[0][..]),
            (0, 2, &[0x80, 0][..]),
            (0, 3, &[0x90, 0, 0][..]),
            (127, 1, &[127][..]),
            (4095, 2, &[0x8f, 0xff][..]),
            (65535, 3, &[0x90, 0xff, 0xff][..]),
        ] {
            let token = FeatureReferenceToken::with_width(value, width).unwrap();
            assert_eq!(token.value(), value);
            assert_eq!(token.raw(), raw);
            assert_eq!(FeatureReferenceToken::read(raw), Some(token));
        }
        for (value, width) in [
            (0, 0),
            (0, 4),
            (128, 1),
            (4096, 2),
            (65536, 3),
            (0, u64::MAX),
        ] {
            assert!(FeatureReferenceToken::with_width(value, width).is_none());
        }
    }
}
