// SPDX-License-Identifier: Apache-2.0
//! Nonempty printable ASCII values shared by parsed and retained records.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub(crate) struct PrintableString<S>(S);

impl<S: AsRef<str>> PrintableString<S> {
    pub(crate) fn new(value: S) -> Result<Self, &'static str> {
        let text = value.as_ref();
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_graphic() || byte == b' ') {
            return Err("value: must be nonempty printable ASCII");
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str { self.0.as_ref() }

    pub(crate) fn into_inner(self) -> S { self.0 }
}

impl PrintableString<&str> {
    pub(crate) fn into_owned(self) -> PrintableString<String> {
        PrintableString(self.0.to_owned())
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
        assert_eq!(serde_json::from_str::<PrintableString<String>>(&json).unwrap(), value);
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
}
