// SPDX-License-Identifier: Apache-2.0
//! Nonempty Unicode attribute values and validated borrowed UTF-16 lanes.

use cadmpeg_core::decode::View;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct UnicodeValue(String);

impl UnicodeValue {
    pub(crate) fn new(value: String) -> Result<Self, &'static str> {
        if value.is_empty() {
            return Err("value must contain at least one Unicode scalar");
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for UnicodeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct UnicodeLane<'a>(&'a [u8]);

impl<'a> UnicodeLane<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Option<Self> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
            return None;
        }
        let mut high_surrogate = false;
        for bytes in bytes.chunks_exact(2) {
            let unit = View::u16_be_at(bytes, 0)?;
            if high_surrogate {
                (0xdc00..=0xdfff).contains(&unit).then_some(())?;
                high_surrogate = false;
            } else if (0xd800..=0xdbff).contains(&unit) {
                high_surrogate = true;
            } else {
                (!(0xdc00..=0xdfff).contains(&unit)).then_some(())?;
            }
        }
        (!high_surrogate).then_some(Self(bytes))
    }

    pub(crate) fn materialize(self) -> Option<UnicodeValue> {
        let code_units = self
            .0
            .chunks_exact(2)
            .map(|bytes| View::u16_be_at(bytes, 0))
            .collect::<Option<Vec<_>>>()?;
        String::from_utf16(&code_units).ok().map(UnicodeValue)
    }
}

#[cfg(test)]
mod tests {
    use super::{UnicodeLane, UnicodeValue};

    #[test]
    fn unicode_values_preserve_scalars_and_reject_empty_or_incomplete_lanes() {
        let value = UnicodeValue::new("\0μ🚀".to_string()).unwrap();
        let wire = serde_json::to_string(value.as_str()).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        assert_eq!(serde_json::from_str::<UnicodeValue>(&wire).unwrap(), value);
        assert!(serde_json::from_str::<UnicodeValue>("\"\"").is_err());
        let bytes = value
            .as_str()
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            UnicodeLane::new(&bytes).unwrap().materialize().unwrap(),
            value
        );
        for bytes in [
            &[][..],
            &[0][..],
            &[0xd8, 0][..],
            &[0xdc, 0][..],
            &[0xd8, 0, 0, 0][..],
        ] {
            assert!(UnicodeLane::new(bytes).is_none());
        }
    }
}
