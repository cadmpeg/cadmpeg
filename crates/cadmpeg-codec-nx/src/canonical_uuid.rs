// SPDX-License-Identifier: Apache-2.0
//! Canonical lowercase UUID text retained from OM frames.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CanonicalUuid<S>(S);

impl<S: crate::immutable_text::ImmutableText> CanonicalUuid<S> {
    pub(crate) fn new(value: S) -> Result<Self, &'static str> {
        let text = value.as_ref();
        if text.len() != 36
            || !text.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                }
            })
        {
            return Err("uuid: must be canonical lowercase UUID text");
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl<S: crate::immutable_text::ImmutableText> serde::Serialize for CanonicalUuid<S> {
    fn serialize<T: serde::Serializer>(&self, serializer: T) -> Result<T::Ok, T::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for CanonicalUuid<String> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::CanonicalUuid;

    #[test]
    fn canonical_text_has_unchanged_string_wire() {
        for text in [
            "01234567-89ab-cdef-0123-456789abcdef",
            "00000000-0000-0000-0000-000000000000",
        ] {
            let value = CanonicalUuid::new(text.to_owned()).unwrap();
            let json = serde_json::to_string(text).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<CanonicalUuid<String>>(&json).unwrap(),
                value
            );
        }
    }

    #[test]
    fn deserialization_rejects_noncanonical_text() {
        for text in [
            "",
            "01234567-89AB-cdef-0123-456789abcdef",
            "01234567-89ab-cdef-0123_456789abcdef",
            "01234567-89ab-cdef-0123-456789abcdeg",
            "01234567-89ab-cdef-0123-456789abcde",
        ] {
            let json = serde_json::to_string(text).unwrap();
            assert!(serde_json::from_str::<CanonicalUuid<String>>(&json)
                .unwrap_err()
                .to_string()
                .contains("uuid"));
        }
    }
    #[test]
    fn canonicaluuid_serializes_the_checked_borrowed_and_owned_text() {
        let text = "01234567-89ab-cdef-0123-456789abcdef";
        let borrowed = super::CanonicalUuid::new(text).unwrap();
        let owned = super::CanonicalUuid::new(text.to_owned()).unwrap();
        for _ in 0..3 {
            assert_eq!(borrowed.as_str(), text);
            assert_eq!(owned.as_str(), text);
            assert_eq!(serde_json::to_string(&borrowed).unwrap(), serde_json::to_string(text).unwrap());
            assert_eq!(serde_json::to_string(&owned).unwrap(), serde_json::to_string(text).unwrap());
        }
    }
}
