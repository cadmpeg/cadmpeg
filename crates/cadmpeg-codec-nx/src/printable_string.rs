// SPDX-License-Identifier: Apache-2.0
//! Nonempty printable ASCII values shared by parsed and retained records.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrintableString<S>(S);

impl<S: crate::immutable_text::ImmutableText> PrintableString<S> {
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
            ctx.next_charged(chars, "NX printable string syntax")
        })?
        .map(|()| Self(value)))
    }

    fn validate<E>(
        text: &str,
        mut next: impl FnMut(&mut std::str::Chars<'_>) -> Result<Option<char>, E>,
    ) -> Result<Result<(), &'static str>, E> {
        if text.is_empty() {
            return Ok(Err("value: must be nonempty printable ASCII"));
        }
        let mut chars = text.chars();
        while let Some(ch) = next(&mut chars)? {
            if !(ch.is_ascii_graphic() || ch == ' ') {
                return Ok(Err("value: must be nonempty printable ASCII"));
            }
        }
        Ok(Ok(()))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn into_inner(self) -> S {
        self.0
    }
}

impl PrintableString<&str> {
    pub(crate) fn try_into_owned_for_decode(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<PrintableString<String>, cadmpeg_core::CodecError> {
        Ok(PrintableString(ctx.copy_retained_text(self.0, operation)?))
    }

    #[cfg(test)]
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

    #[test]
    fn printable_string_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        for text in ["Name", "Nameμ"] {
            let error = crate::test_support::resource_refusal_at(
                &[],
                ResourceDimension::WorkUnits,
                "NX printable string syntax",
                |ctx| PrintableString::from_wire(ctx, text).map(|value| value.is_ok()),
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX printable string syntax"));
        }
    }

    #[test]
    fn validation_stops_before_an_unread_suffix() {
        let text = format!("\n{}", "a".repeat(4096));
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 1,
            |ctx| {
                assert!(PrintableString::from_wire(ctx, text.as_str())
                    .unwrap()
                    .is_err());
                assert!(PrintableString::new(text.as_str()).is_err());
            },
        );
    }

    #[test]
    fn printable_string_checked_copy_pays_one_exact_copy() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;

        let value = PrintableString::new("Name").unwrap();
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 4,
            |ctx| {
                let owned = value
                    .try_into_owned_for_decode(ctx, "NX checked printable string copy")
                    .unwrap();
                assert_eq!(owned.as_str(), "Name");
                assert_eq!(ctx.resource_refusal(), None);
            },
        );

        for dimension in [
            ResourceDimension::WorkUnits,
            ResourceDimension::RetainedBytes,
        ] {
            let error = crate::test_support::resource_refusal_at(
                &[],
                dimension,
                "NX checked printable string copy",
                |ctx| value.try_into_owned_for_decode(ctx, "NX checked printable string copy"),
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == dimension
                    && limit.operation == "NX checked printable string copy"
                    && limit.additional == 4));
        }
    }
}
