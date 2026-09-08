// SPDX-License-Identifier: Apache-2.0
//! Printable diagnostic text in a byte-length operation-state message frame.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::printable_string::PrintableString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateMessageText<S>(PrintableString<S>);

impl<S: AsRef<str>> StateMessageText<S> {
    pub(crate) fn new(text: S) -> Result<Self, &'static str> {
        let text =
            PrintableString::new(text).map_err(|_| "text: must be nonempty printable ASCII")?;
        if text.as_str().len() > usize::from(u8::MAX) - 2 {
            return Err("text: length plus two must fit declared_length");
        }
        Ok(Self(text))
    }

    pub(crate) fn declared_length(&self) -> u8 {
        self.0.as_str().len() as u8 + 2
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl StateMessageText<&str> {
    pub(crate) fn into_owned(self) -> StateMessageText<String> {
        StateMessageText(self.0.into_owned())
    }
}

impl<S: AsRef<str>> Serialize for StateMessageText<S> {
    fn serialize<T: Serializer>(&self, serializer: T) -> Result<T::Ok, T::Error> {
        let mut state = serializer.serialize_struct("StateMessageText", 2)?;
        state.serialize_field("declared_length", &self.declared_length())?;
        state.serialize_field("text", self.as_str())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for StateMessageText<String> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            declared_length: u8,
            text: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        let text = Self::new(wire.text).map_err(serde::de::Error::custom)?;
        if wire.declared_length != text.declared_length() {
            return Err(serde::de::Error::custom(
                "declared_length: disagrees with text length plus two",
            ));
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::StateMessageText;

    #[test]
    fn message_text_derives_length_and_preserves_spaces() {
        for text in [" ".to_string(), "x".repeat(253)] {
            let value = StateMessageText::new(text.as_str()).unwrap().into_owned();
            assert_eq!(usize::from(value.declared_length()), text.len() + 2);
            let json = format!(
                r#"{{"declared_length":{},"text":{}}}"#,
                text.len() + 2,
                serde_json::to_string(&text).unwrap()
            );
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<StateMessageText<String>>(&json).unwrap(),
                value
            );
        }
    }

    #[test]
    fn message_text_rejects_invalid_bytes_lengths_and_duplicate_count() {
        for text in [
            String::new(),
            "\n".to_string(),
            "μ".to_string(),
            "x".repeat(254),
        ] {
            assert!(StateMessageText::new(text).is_err());
        }
        for length in [0, 1, 2, 4, 255] {
            let json = format!(r#"{{"declared_length":{length},"text":"A"}}"#);
            assert!(serde_json::from_str::<StateMessageText<String>>(&json)
                .unwrap_err()
                .to_string()
                .contains("declared_length"));
        }
    }
}
