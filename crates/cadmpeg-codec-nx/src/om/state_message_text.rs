// SPDX-License-Identifier: Apache-2.0
//! Printable diagnostic text in a byte-length operation-state message frame.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::printable_string::PrintableString;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateMessageText<S>(PrintableString<S>, u8);

impl<S: crate::immutable_text::ImmutableText> StateMessageText<S> {
    pub(super) fn new(text: S) -> Result<Self, &'static str> {
        let text =
            PrintableString::new(text).map_err(|_| "text: must be nonempty printable ASCII")?;
        Self::from_printable(text)
    }

    pub(super) fn from_wire(ctx: &DecodeContext<'_>, text: S) -> Result<Result<Self, &'static str>, CodecError> {
        Ok(PrintableString::from_wire(ctx, text)?
            .map_err(|_| "text: must be nonempty printable ASCII")
            .and_then(Self::from_printable))
    }

    fn from_printable(text: PrintableString<S>) -> Result<Self, &'static str> {
        if text.as_str().len() > usize::from(u8::MAX) - 2 {
            return Err("text: length plus two must fit declared_length");
        }
        let count = u8::try_from(text.as_str().len())
            .map_err(|_| "text: length plus two must fit declared_length")?
            + 2;
        Ok(Self(text, count))
    }

    pub(super) fn declared_length(&self) -> u8 {
        self.1
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl StateMessageText<&str> {
    pub(super) fn into_owned(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<StateMessageText<String>, CodecError> {
        let text = self.as_str();
        let mut owned = String::new();
        ctx.append_retained(&mut owned, text, "NX state message text")?;
        Ok(StateMessageText(
            PrintableString::from_wire(ctx, owned)?.map_err(CodecError::malformed)?,
            self.1,
        ))
    }
}

impl<S: crate::immutable_text::ImmutableText> Serialize for StateMessageText<S> {
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
    fn retained_state_message_text_iteration_refusal_propagates() {
        let text = StateMessageText::new("NX").unwrap();
        crate::test_support::with_decode_context_over(
            &[],
            // The append reads two bytes before the constructor validates them.
            |policy| policy.limits.max_work_units = 2,
            |ctx| {
                let error = text.into_owned(ctx).unwrap_err();
                let cadmpeg_core::CodecError::ResourceLimit(limit) = error else { panic!("text validation must refuse"); };
                assert_eq!(limit.operation, "NX printable string syntax");
                assert_eq!(ctx.resource_refusal(), Some(limit));
            },
        );
    }

    #[test]
    fn message_text_copy_preserves_resource_refusals() {
        for retained in [false, true] {
            crate::test_support::with_decode_context_over(
                b"NX",
                |policy| {
                    if retained {
                        policy.limits.max_retained_bytes = 1;
                    } else {
                        policy.limits.max_work_units = 1;
                    }
                },
                |ctx| {
                    let error = StateMessageText::new("NX").unwrap().into_owned(ctx).unwrap_err();
                    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                        panic!("text copy must return the resource refusal");
                    };
                    assert_eq!(limit.operation, "NX state message text");
                    assert_eq!(
                        limit.dimension,
                        if retained {
                            cadmpeg_core::decode::ResourceDimension::RetainedBytes
                        } else {
                            cadmpeg_core::decode::ResourceDimension::WorkUnits
                        }
                    );
                    assert_eq!(ctx.resource_refusal(), Some(limit));
                },
            );
        }
    }

    #[test]
    fn message_text_derives_length_and_preserves_spaces() {
        for text in [" ".to_string(), "x".repeat(253)] {
            let value = crate::test_support::with_decode_context(|ctx| {
                StateMessageText::new(text.as_str())
                    .unwrap()
                    .into_owned(ctx)
            })
            .unwrap();
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
