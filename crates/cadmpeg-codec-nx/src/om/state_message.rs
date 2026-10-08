// SPDX-License-Identifier: Apache-2.0
//! Operation-state diagnostic bodies and their source frames.

use super::state_message_text::StateMessageText;
use super::state_tagged_value::StateTaggedValue;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StateMessageSeverity {
    Alert,
    Failure,
}

impl StateMessageSeverity {
    pub(crate) fn from_word(word: u16) -> Option<Self> {
        match word >> 8 {
            0x01 => Some(Self::Alert),
            0x03 => Some(Self::Failure),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateMessage<S> {
    pub(crate) text: StateMessageText<S>,
    pub(crate) value: StateTaggedValue,
    pub(crate) count_or_severity: u16,
}

impl<S: crate::immutable_text::ImmutableText> StateMessage<S> {
    pub(super) fn byte_len(&self) -> usize {
        usize::from(self.text.declared_length()) + 7 + self.value.raw().len()
    }
    pub(crate) fn severity(&self) -> Option<StateMessageSeverity> {
        StateMessageSeverity::from_word(self.count_or_severity)
    }
}

impl StateMessage<&str> {
    pub(crate) fn into_owned(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<StateMessage<String>, CodecError> {
        Ok(StateMessage {
            text: self.text.into_owned(ctx)?,
            value: self.value,
            count_or_severity: self.count_or_severity,
        })
    }
}

impl<S: crate::immutable_text::ImmutableText> Serialize for StateMessage<S> {
    fn serialize<T: Serializer>(&self, serializer: T) -> Result<T::Ok, T::Error> {
        let severity = self.severity();
        let mut state =
            serializer.serialize_struct("StateMessage", 6 + usize::from(severity.is_some()))?;
        state.serialize_field("declared_length", &self.text.declared_length())?;
        state.serialize_field("text", self.text.as_str())?;
        state.serialize_field("value_marker", &self.value.marker())?;
        state.serialize_field("value", &self.value.value())?;
        state.serialize_field("raw_value", self.value.raw())?;
        state.serialize_field("count_or_severity", &self.count_or_severity)?;
        if let Some(severity) = severity {
            state.serialize_field("severity", &severity)?;
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for StateMessage<String> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            #[serde(flatten)]
            text: StateMessageText<String>,
            #[serde(flatten)]
            value: StateTaggedValue,
            count_or_severity: u16,
            #[serde(default, deserialize_with = "deserialize_severity")]
            severity: Option<StateMessageSeverity>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let body = Self {
            text: wire.text,
            value: wire.value,
            count_or_severity: wire.count_or_severity,
        };
        if wire.severity != body.severity() {
            return Err(serde::de::Error::custom(
                "operation-state message severity disagrees with count_or_severity",
            ));
        }
        Ok(body)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationStateMessage<'a> {
    offset: usize,
    body: StateMessage<&'a str>,
}

impl<'a> OperationStateMessage<'a> {
    pub(super) fn read(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        bytes: &'a [u8],
        at: usize,
        base: usize,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if bytes.get(at) != Some(&3) {
            return Ok(None);
        }
        let Some(declared_length) = at
            .checked_add(1)
            .and_then(|offset| bytes.get(offset))
            .copied()
        else {
            return Ok(None);
        };
        let Some(text_end) = at.checked_add(usize::from(declared_length)) else {
            return Ok(None);
        };
        let Some(raw) = at
            .checked_add(2)
            .and_then(|start| bytes.get(start..text_end))
        else {
            return Ok(None);
        };
        let Ok(value) = ctx.validate_utf8(raw, "NX state message UTF-8 validation")? else {
            return Ok(None);
        };
        let Ok(text) = StateMessageText::from_wire(ctx, value)? else {
            return Ok(None);
        };
        let Some(zeros_end) = text_end.checked_add(5) else {
            return Ok(None);
        };
        if bytes.get(text_end..zeros_end) != Some(&[0; 5]) {
            return Ok(None);
        }
        let Some(value) = StateTaggedValue::read_at(bytes, zeros_end) else {
            return Ok(None);
        };
        let Some(count_at) = zeros_end.checked_add(value.raw().len()) else {
            return Ok(None);
        };
        let Some(count_or_severity) = View::u16_be_at(bytes, count_at) else {
            return Ok(None);
        };
        let Some(offset) = base.checked_add(at) else {
            return Ok(None);
        };
        Ok(Self::new(
            offset,
            StateMessage {
                text,
                value,
                count_or_severity,
            },
        ))
    }
    pub(super) fn new(offset: usize, body: StateMessage<&'a str>) -> Option<Self> {
        offset.checked_add(body.byte_len())?;
        Some(Self { offset, body })
    }
    pub(crate) fn offset(self) -> usize {
        self.offset
    }
    pub(super) fn end_offset(self) -> usize {
        self.offset + self.body.byte_len()
    }
    pub(crate) fn body(self) -> StateMessage<&'a str> {
        self.body
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationStateMessage, StateMessage};

    #[test]
    fn state_message_text_iteration_refusal_propagates() {
        let bytes = [3, 3, b'A', 0, 0, 0, 0, 0, 0xa0, 0, 0, 0, 0];
        let error = crate::test_support::resource_refusal_at(
            &bytes,
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "NX printable string syntax",
            |ctx| OperationStateMessage::read(ctx, &bytes, 0, 0),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "NX printable string syntax")
        );
    }

    #[test]
    fn source_extent_follows_text_and_each_tagged_width() {
        for token in [&[0xa0, 0, 0][..], &[0xc0, 0, 0, 0], &[0xe0, 0, 0, 0, 0]] {
            let mut bytes = vec![3, 3, b'A', 0, 0, 0, 0, 0];
            bytes.extend_from_slice(token);
            bytes.extend([0, 0]);
            let row = crate::test_support::with_decode_context(|ctx| {
                OperationStateMessage::read(ctx, &bytes, 0, 100)
            })
            .unwrap()
            .unwrap();
            assert_eq!(row.end_offset(), 110 + token.len());
            assert!(
                crate::test_support::with_decode_context(|ctx| OperationStateMessage::read(
                    ctx,
                    &bytes,
                    0,
                    usize::MAX - bytes.len()
                ))
                .unwrap()
                .is_some()
            );
            assert!(
                crate::test_support::with_decode_context(|ctx| OperationStateMessage::read(
                    ctx,
                    &bytes,
                    0,
                    usize::MAX - bytes.len() + 1
                ))
                .unwrap()
                .is_none()
            );
        }
    }

    #[test]
    fn wire_severity_is_derived_and_preserves_field_order() {
        let json = r#"{"declared_length":3,"text":"A","value_marker":160,"value":0,"raw_value":[160,0,0],"count_or_severity":256,"severity":"alert"}"#;
        let body: StateMessage<String> = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&body).unwrap(), json);
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["severity"] = "failure".into();
        assert!(serde_json::from_value::<StateMessage<String>>(wire)
            .unwrap_err()
            .to_string()
            .contains("severity"));
    }

    #[test]
    fn state_message_wire_refuses_a_null_severity() {
        let json = r#"{"declared_length":3,"text":"A","value_marker":160,"value":0,"raw_value":[160,0,0],"count_or_severity":256,"severity":null}"#;
        let error = serde_json::from_str::<StateMessage<String>>(json)
            .expect_err("a null severity is not a spelling of an absent severity");
        assert!(error.to_string().contains("null"), "{error}");
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_severity, StateMessageSeverity, "severity");
