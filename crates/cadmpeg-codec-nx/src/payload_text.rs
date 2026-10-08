// SPDX-License-Identifier: Apache-2.0
//! Nonempty Unicode text without control characters.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayloadText<S>(S);

impl<S: crate::immutable_text::ImmutableText> PayloadText<S> {
    pub(crate) fn new(value: S) -> Result<Self, &'static str> {
        match Self::validate(value.as_ref(), |chars| {
            Ok::<_, std::convert::Infallible>(chars.next())
        }) {
            Ok(valid) => valid?,
            Err(error) => match error {},
        }
        Ok(Self(value))
    }

    pub(crate) fn from_wire(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: S,
    ) -> Result<Result<Self, &'static str>, cadmpeg_core::CodecError> {
        Ok(Self::validate(value.as_ref(), |chars| {
            ctx.next_charged(chars, "NX payload text syntax")
        })?
        .map(|()| Self(value)))
    }

    fn validate<E>(
        text: &str,
        mut next: impl FnMut(&mut std::str::Chars<'_>) -> Result<Option<char>, E>,
    ) -> Result<Result<(), &'static str>, E> {
        if text.is_empty() {
            return Ok(Err(
                "value: must be nonempty text without control characters",
            ));
        }
        let mut chars = text.chars();
        while let Some(ch) = next(&mut chars)? {
            if ch.is_control() {
                return Ok(Err(
                    "value: must be nonempty text without control characters",
                ));
            }
        }
        Ok(Ok(()))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl PayloadText<&str> {
    pub(crate) fn try_into_owned_for_decode(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<PayloadText<String>, cadmpeg_core::CodecError> {
        Ok(PayloadText(ctx.copy_retained_text(self.0, operation)?))
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
                &[],
                ResourceDimension::WorkUnits,
                "NX payload text syntax",
                |ctx| PayloadText::from_wire(ctx, text).map(|value| value.is_ok()),
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX payload text syntax"));
        }
    }

    #[test]
    fn validation_stops_before_an_unread_suffix() {
        let text = format!("\n{}", "a".repeat(4096));
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 1,
            |ctx| {
                assert!(PayloadText::from_wire(ctx, text.as_str()).unwrap().is_err());
                assert!(PayloadText::new(text.as_str()).is_err());
            },
        );
    }

    #[test]
    fn payload_text_checked_copy_pays_one_exact_copy() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;

        let value = PayloadText::new("μ").unwrap();
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 2,
            |ctx| {
                let owned = value
                    .try_into_owned_for_decode(ctx, "NX checked payload text copy")
                    .unwrap();
                assert_eq!(owned.as_str(), "μ");
                assert_eq!(ctx.resource_refusal(), None);
            },
        );

        for dimension in [ResourceDimension::WorkUnits, ResourceDimension::RetainedBytes] {
            let error = crate::test_support::resource_refusal_at(
                &[],
                dimension,
                "NX checked payload text copy",
                |ctx| value.try_into_owned_for_decode(ctx, "NX checked payload text copy"),
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == dimension
                    && limit.operation == "NX checked payload text copy"
                    && limit.additional == 2));
        }
    }
}
