// SPDX-License-Identifier: Apache-2.0
//! Complete operation-state tagged integer tokens.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde::ser::SerializeStruct;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenBytes {
    Short([u8; 3]),
    Medium([u8; 4]),
    Wide([u8; 5]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateTaggedValue(TokenBytes);

impl StateTaggedValue {
    pub(crate) fn read_at(bytes: &[u8], at: usize) -> Option<Self> {
        let token = match bytes.get(at..)? {
            [marker @ 0xa0..=0xbf, a, b, ..] => TokenBytes::Short([*marker, *a, *b]),
            [marker @ 0xc0..=0xdf, a, b, c, ..] => TokenBytes::Medium([*marker, *a, *b, *c]),
            [marker @ (0xe0 | 0xff), a, b, c, d, ..] => TokenBytes::Wide([*marker, *a, *b, *c, *d]),
            _ => return None,
        };
        Some(Self(token))
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match &self.0 {
            TokenBytes::Short(bytes) => bytes,
            TokenBytes::Medium(bytes) => bytes,
            TokenBytes::Wide(bytes) => bytes,
        }
    }

    pub(crate) fn marker(self) -> u8 {
        match self.0 {
            TokenBytes::Short([marker, ..]) | TokenBytes::Medium([marker, ..]) | TokenBytes::Wide([marker, ..]) => marker,
        }
    }

    pub(crate) fn value(self) -> u32 {
        match self.0 {
            TokenBytes::Short([marker, a, b]) => (u32::from(marker - 0xa0) << 16) | (u32::from(a) << 8) | u32::from(b),
            TokenBytes::Medium([marker, a, b, c]) => (u32::from(marker - 0xc0) << 24) | (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c),
            TokenBytes::Wide([_, a, b, c, d]) => (u32::from(a) << 24) | (u32::from(b) << 16) | (u32::from(c) << 8) | u32::from(d),
        }
    }
}

impl Serialize for StateTaggedValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("StateTaggedValue", 3)?;
        state.serialize_field("value_marker", &self.marker())?;
        state.serialize_field("value", &self.value())?;
        state.serialize_field("raw_value", self.raw())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for StateTaggedValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            value_marker: u8,
            value: u32,
            raw_value: Vec<u8>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self::read_at(&wire.raw_value, 0)
            .filter(|value| value.raw().len() == wire.raw_value.len())
            .ok_or_else(|| serde::de::Error::custom("raw_value: invalid tagged integer token"))?;
        if value.marker() != wire.value_marker {
            return Err(serde::de::Error::custom("value_marker: disagrees with raw_value"));
        }
        if value.value() != wire.value {
            return Err(serde::de::Error::custom("value: disagrees with raw_value"));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::StateTaggedValue;

    #[test]
    fn tagged_wire_preserves_width_marker_and_value() {
        for json in [
            r#"{"value_marker":191,"value":2097151,"raw_value":[191,255,255]}"#,
            r#"{"value_marker":223,"value":536870911,"raw_value":[223,255,255,255]}"#,
            r#"{"value_marker":224,"value":4294967295,"raw_value":[224,255,255,255,255]}"#,
            r#"{"value_marker":255,"value":4294967295,"raw_value":[255,255,255,255,255]}"#,
        ] {
            let value: StateTaggedValue = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
        }
    }

    #[test]
    fn tagged_wire_rejects_incomplete_tokens_and_inconsistent_projections() {
        for (json, field) in [
            (r#"{"value_marker":160,"value":0,"raw_value":[]}"#, "raw_value"),
            (r#"{"value_marker":160,"value":0,"raw_value":[160,0]}"#, "raw_value"),
            (r#"{"value_marker":160,"value":0,"raw_value":[160,0,0,0]}"#, "raw_value"),
            (r#"{"value_marker":225,"value":0,"raw_value":[225,0,0,0,0]}"#, "raw_value"),
            (r#"{"value_marker":255,"value":0,"raw_value":[224,0,0,0,0]}"#, "value_marker"),
            (r#"{"value_marker":160,"value":1,"raw_value":[160,0,0]}"#, "value"),
        ] {
            assert!(serde_json::from_str::<StateTaggedValue>(json)
                .unwrap_err().to_string().contains(field));
        }
    }
}
