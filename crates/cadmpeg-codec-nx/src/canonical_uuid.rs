// SPDX-License-Identifier: Apache-2.0
//! Canonical lowercase UUID text retained from OM frames.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub(crate) struct CanonicalUuid<S>(S);

impl<S: AsRef<str>> CanonicalUuid<S> {
    pub(crate) fn new(value: S) -> Result<Self, &'static str> {
        let text = value.as_ref();
        if text.len() != 36 || !text.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        }) {
            return Err("uuid: must be canonical lowercase UUID text");
        }
        Ok(Self(value))
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str { self.0.as_ref() }
}

impl CanonicalUuid<&str> {
    pub(crate) fn into_owned(self) -> CanonicalUuid<String> {
        CanonicalUuid(self.0.to_owned())
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
        for text in ["01234567-89ab-cdef-0123-456789abcdef", "00000000-0000-0000-0000-000000000000"] {
            let value = CanonicalUuid::new(text).unwrap().into_owned();
            let json = serde_json::to_string(text).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
            assert_eq!(serde_json::from_str::<CanonicalUuid<String>>(&json).unwrap(), value);
        }
    }

    #[test]
    fn deserialization_rejects_noncanonical_text() {
        for text in ["", "01234567-89AB-cdef-0123-456789abcdef", "01234567-89ab-cdef-0123_456789abcdef", "01234567-89ab-cdef-0123-456789abcdeg", "01234567-89ab-cdef-0123-456789abcde"] {
            let json = serde_json::to_string(text).unwrap();
            assert!(serde_json::from_str::<CanonicalUuid<String>>(&json)
                .unwrap_err().to_string().contains("uuid"));
        }
    }
}
