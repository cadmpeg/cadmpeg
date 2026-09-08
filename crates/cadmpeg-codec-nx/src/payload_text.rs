// SPDX-License-Identifier: Apache-2.0
//! Nonempty Unicode text without control characters.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub(crate) struct PayloadText<S>(S);

impl<S: AsRef<str>> PayloadText<S> {
    pub(crate) fn new(value: S) -> Result<Self, &'static str> {
        let text = value.as_ref();
        if text.is_empty() || text.chars().any(char::is_control) {
            return Err("value: must be nonempty text without control characters");
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl PayloadText<&str> {
    pub(crate) fn into_owned(self) -> PayloadText<String> {
        PayloadText(self.0.to_owned())
    }
}

impl<'de> serde::Deserialize<'de> for PayloadText<String> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::PayloadText;

    #[test]
    fn payload_text_preserves_wire_and_owned_transfer() {
        let text = " ×Name42";
        let value = PayloadText::new(text).unwrap().into_owned();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, serde_json::to_string(text).unwrap());
        assert_eq!(
            serde_json::from_str::<PayloadText<String>>(&json).unwrap(),
            value
        );
    }

    #[test]
    fn payload_text_rejects_empty_and_control() {
        for text in ["", "\0", "\n", "\t", "\u{7f}"] {
            assert!(PayloadText::new(text).is_err());
            let json = serde_json::to_string(text).unwrap();
            let error = serde_json::from_str::<PayloadText<String>>(&json).unwrap_err();
            assert!(error.to_string().contains("value"));
        }
    }
}
