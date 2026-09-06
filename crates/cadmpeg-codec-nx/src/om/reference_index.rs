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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
