// SPDX-License-Identifier: Apache-2.0
//! Nonempty printable ASCII values shared by parsed and retained records.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrintableString<S>(S);

impl<S: crate::immutable_text::ImmutableText> PrintableString<S> {
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
            ctx.admit_iter(text, "NX printable string syntax")
        })?.map(|()| Self(value)))
    }

    fn validate<'a, E, I: Iterator<Item = char>>(
        text: &'a str,
        admit: impl FnOnce(&'a str) -> Result<I, E>,
    ) -> Result<Result<(), &'static str>, E> {
        if text.is_empty() || !admit(text)?.all(|ch| ch.is_ascii_graphic() || ch == ' ') {
            return Ok(Err("value: must be nonempty printable ASCII"));
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
                &[], ResourceDimension::WorkUnits, "NX printable string syntax",
                |ctx| PrintableString::from_wire(ctx, text).map(|value| value.is_ok()),
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX printable string syntax"));
        }
    }
}
