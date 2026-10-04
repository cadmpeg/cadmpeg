// SPDX-License-Identifier: Apache-2.0
//! Nonempty Unicode text without control characters.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayloadText<S>(S);

impl<S: crate::immutable_text::ImmutableText> PayloadText<S> {
    pub(crate) fn new(value: S) -> Result<Self, &'static str> {
        match Self::validate(value.as_ref(), |text| {
            Ok::<_, std::convert::Infallible>(text.chars())
        }) {
            Ok(valid) => valid?,
            Err(error) => match error {},
        }
        Ok(Self(value))
    }

    pub(crate) fn from_wire(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>, value: S,
    ) -> Result<Result<Self, &'static str>, cadmpeg_core::CodecError> {
        Ok(Self::validate(value.as_ref(), |text| {
            ctx.admit_iter(text, "NX payload text syntax")
        })?.map(|()| Self(value)))
    }

    fn validate<'a, E, I: Iterator<Item = char>>(
        text: &'a str,
        admit: impl FnOnce(&'a str) -> Result<I, E>,
    ) -> Result<Result<(), &'static str>, E> {
        if text.is_empty() || admit(text)?.any(char::is_control) {
            return Ok(Err("value: must be nonempty text without control characters"));
        }
        Ok(Ok(()))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

#[cfg(test)]
impl PayloadText<&str> {
    pub(crate) fn into_owned(self) -> PayloadText<String> {
        PayloadText(self.0.to_owned())
    }
}

impl<S: crate::immutable_text::ImmutableText> serde::Serialize for PayloadText<S> {
    fn serialize<T: serde::Serializer>(&self, serializer: T) -> Result<T::Ok, T::Error> {
        serializer.serialize_str(self.as_str())
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
    #[test]
    fn payloadtext_serializes_the_checked_borrowed_and_owned_text() {
        let text = "μ Name";
        let borrowed = super::PayloadText::new(text).unwrap();
        let owned = super::PayloadText::new(text.to_owned()).unwrap();
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

    #[test]
    fn payload_text_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        for text in ["Name", "Name\n"] {
            let error = crate::test_support::resource_refusal_at(
                &[], ResourceDimension::WorkUnits, "NX payload text syntax",
                |ctx| PayloadText::from_wire(ctx, text).map(|value| value.is_ok()),
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX payload text syntax"));
        }
    }
}
