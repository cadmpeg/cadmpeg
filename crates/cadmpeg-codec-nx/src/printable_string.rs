// SPDX-License-Identifier: Apache-2.0
//! Nonempty printable ASCII values shared by parsed and retained records.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrintableString<S>(S);

const NOT_PRINTABLE: &str = "value: must be nonempty printable ASCII";

fn printable(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
}

impl<S: crate::immutable_text::ImmutableText> PrintableString<S> {
    pub(crate) fn new(value: S) -> Result<Self, &'static str> {
        if !printable(value.as_ref().as_bytes()) {
            return Err(NOT_PRINTABLE);
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn into_inner(self) -> S {
        self.0
    }
}

impl<'a> PrintableString<&'a str> {
    /// Admit raw bytes. The check stops at the first byte that is not
    /// printable ASCII, so it never reads past the end of a printable run.
    pub(crate) fn from_bytes(bytes: &'a [u8]) -> Result<Self, &'static str> {
        if !printable(bytes) {
            return Err(NOT_PRINTABLE);
        }
        std::str::from_utf8(bytes)
            .map(Self)
            .map_err(|_| NOT_PRINTABLE)
    }

    pub(crate) fn into_owned(self) -> PrintableString<String> {
        PrintableString(self.0.to_owned())
    }
}

impl<S: crate::immutable_text::ImmutableText> serde::Serialize for PrintableString<S> {
    fn serialize<T: serde::Serializer>(&self, serializer: T) -> Result<T::Ok, T::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for PrintableString<String> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::PrintableString;

    #[test]
    fn printable_value_preserves_wire_and_owned_transfer() {
        let text = " ~Name42";
        let value = PrintableString::new(text).unwrap().into_owned();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, serde_json::to_string(text).unwrap());
        assert_eq!(
            serde_json::from_str::<PrintableString<String>>(&json).unwrap(),
            value
        );
    }

    #[test]
    fn printable_value_rejects_empty_control_and_non_ascii() {
        for text in ["", "\0", "\n", "\t", "\u{7f}", "μ"] {
            assert!(PrintableString::new(text).is_err());
            let json = serde_json::to_string(text).unwrap();
            let error = serde_json::from_str::<PrintableString<String>>(&json).unwrap_err();
            assert!(error.to_string().contains("value"));
        }
    }

    #[test]
    fn byte_admission_matches_decoding_then_admitting_text() {
        let alphabet = [
            0x00, 0x09, 0x1f, b' ', b'A', b'~', 0x7f, 0x80, 0xc2, 0xb5, 0xff,
        ];
        let mut inputs = vec![Vec::new()];
        let mut frontier = vec![Vec::new()];
        for _ in 0..3 {
            frontier = frontier
                .iter()
                .flat_map(|prefix: &Vec<u8>| {
                    alphabet
                        .iter()
                        .map(move |byte| [prefix.as_slice(), &[*byte]].concat())
                })
                .collect();
            inputs.extend(frontier.iter().cloned());
        }
        for input in &inputs {
            let decoded = std::str::from_utf8(input)
                .ok()
                .and_then(|text| PrintableString::new(text).ok());
            assert_eq!(PrintableString::from_bytes(input).ok(), decoded);
        }
    }
    #[test]
    fn printablestring_serializes_the_checked_borrowed_and_owned_text() {
        let text = "Name";
        let borrowed = super::PrintableString::new(text).unwrap();
        let owned = super::PrintableString::new(text.to_owned()).unwrap();
        for _ in 0..3 {
            assert_eq!(borrowed.as_str(), text);
            assert_eq!(owned.as_str(), text);
            assert_eq!(
                serde_json::to_string(&borrowed).unwrap(),
                serde_json::to_string(text).unwrap()
            );
            assert_eq!(
                serde_json::to_string(&owned).unwrap(),
                serde_json::to_string(text).unwrap()
            );
        }
    }
}
