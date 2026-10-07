// SPDX-License-Identifier: Apache-2.0
//! Length-framed NX product text and its source header forms.

use crate::printable_string::PrintableString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductText<S>(PrintableString<S>);

impl<S: crate::immutable_text::ImmutableText> ProductText<S> {
    fn new(value: S) -> Result<Self, &'static str> {
        let value = PrintableString::new(value)
            .map_err(|_| "product_version/version: requires printable ASCII")?;
        Self::from_printable(value)
    }

    fn from_wire(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: S,
    ) -> Result<Result<Self, &'static str>, cadmpeg_core::CodecError> {
        Ok(PrintableString::from_wire(ctx, value)?
            .map_err(|_| "product_version/version: requires printable ASCII")
            .and_then(Self::from_printable))
    }

    fn from_printable(value: PrintableString<S>) -> Result<Self, &'static str> {
        if !value.as_str().starts_with("NX ") || value.as_str().len() > 253 {
            return Err("product_version/version: requires NX-prefixed text of at most 253 bytes");
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl ProductText<&str> {
    pub(crate) fn try_into_owned_for_decode(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<ProductText<String>, cadmpeg_core::CodecError> {
        let value = self.as_str();
        let owned = ctx.copy_retained_text(value, "retain NX store version")?;
        ProductText::from_wire(ctx, owned)?
            .map_err(|_| ctx.refuse_codec_limit("validate NX store version", 0, 1))
    }
}

impl<S: crate::immutable_text::ImmutableText> serde::Serialize for ProductText<S> {
    fn serialize<T: serde::Serializer>(&self, serializer: T) -> Result<T::Ok, T::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ProductText<String> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProductRecordForm {
    Modern,
    LegacyFeature,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProductRecord<'a> {
    form: ProductRecordForm,
    text: ProductText<&'a str>,
}

impl<'a> ProductRecord<'a> {
    pub(crate) fn read(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        bytes: &'a [u8],
        form: ProductRecordForm,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        (|| {
            let (length_offset, text_start): (usize, usize) = match form {
                ProductRecordForm::Modern
                    if matches!(bytes.get(..2), Some([0x04 | 0x05, 0x01])) =>
                {
                    (2, 3)
                }
                ProductRecordForm::LegacyFeature if bytes.first() == Some(&0x01) => (1, 2),
                _ => return None,
            };
            let text_length = usize::from(*bytes.get(length_offset)?).checked_sub(2)?;
            let text_end = text_start.checked_add(text_length)?;
            let text = propagate_resource!(ProductText::from_wire(
                ctx,
                propagate_resource!(ctx.validate_utf8(
                    bytes.get(text_start..text_end)?,
                    "NX product text UTF-8 validation",
                ))
                .ok()?,
            ))
            .ok()?;
            (bytes.get(text_end) == Some(&0)).then_some(Ok(Self { form, text }))
        })()
        .transpose()
    }

    pub(super) fn text(self) -> ProductText<&'a str> {
        self.text
    }

    pub(super) fn byte_len(self) -> usize {
        let header_len = match self.form {
            ProductRecordForm::Modern => 3,
            ProductRecordForm::LegacyFeature => 2,
        };
        header_len + self.text.as_str().len() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::{ProductRecord, ProductRecordForm, ProductText};

    #[test]
    fn product_record_text_iteration_refusal_propagates() {
        for (bytes, form) in [
            (&b"\x04\x01\x05NX \0"[..], ProductRecordForm::Modern),
            (&b"\x05\x01\x05NX \0"[..], ProductRecordForm::Modern),
            (&b"\x01\x05NX \0"[..], ProductRecordForm::LegacyFeature),
        ] {
            let error = crate::test_support::resource_refusal_at(
                bytes,
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                "NX printable string syntax",
                |ctx| ProductRecord::read(ctx, bytes, form),
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "NX printable string syntax")
            );
        }
    }

    #[test]
    fn retained_product_text_iteration_refusal_propagates() {
        let text = ProductText::new("NX ").unwrap();
        crate::test_support::with_decode_context_over(
            &[],
            // The append reads three bytes before the constructor validates them.
            |policy| policy.limits.max_work_units = 3,
            |ctx| {
                let error = text.try_into_owned_for_decode(ctx).unwrap_err();
                let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                    panic!("text validation must refuse");
                };
                assert_eq!(limit.operation, "NX printable string syntax");
                assert_eq!(ctx.resource_refusal(), Some(limit));
            },
        );
    }

    #[test]
    fn product_text_preserves_wire_and_length_bound() {
        let text = format!("NX {}", "x".repeat(250));

        crate::test_support::with_decode_context_over(
            text.as_bytes(),
            |_| {},
            |ctx| {
                let value = ProductText::new(text.as_str())
                    .unwrap()
                    .try_into_owned_for_decode(ctx)
                    .unwrap();
                let wire = serde_json::to_string(&value).unwrap();
                assert_eq!(wire, serde_json::to_string(&text).unwrap());
                assert_eq!(
                    serde_json::from_str::<ProductText<String>>(&wire).unwrap(),
                    value
                );
                for text in ["NX", "NX μ", "NX \n", &format!("NX {}", "x".repeat(251))] {
                    assert!(ProductText::new(text).is_err());
                    let error = serde_json::from_str::<ProductText<String>>(
                        &serde_json::to_string(text).unwrap(),
                    )
                    .unwrap_err();
                    assert!(error.to_string().contains("product_version/version"));
                }
            },
        );
    }

    #[test]
    fn product_frames_derive_lengths_for_both_modern_markers_and_legacy() {
        for marker in [4, 5] {
            let frame = [marker, 1, 5, b'N', b'X', b' ', 0];
            let product = crate::test_support::with_decode_context(|ctx| {
                ProductRecord::read(ctx, &frame, ProductRecordForm::Modern)
            })
            .unwrap()
            .unwrap();
            assert_eq!(product.text().as_str(), "NX ");
            assert_eq!(product.byte_len(), frame.len());
        }
        let frame = [1, 5, b'N', b'X', b' ', 0];
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| ProductRecord::read(
                ctx,
                &frame,
                ProductRecordForm::LegacyFeature
            ))
            .unwrap()
            .unwrap()
            .byte_len(),
            frame.len()
        );
        assert!(
            crate::test_support::with_decode_context(|ctx| ProductRecord::read(
                ctx,
                &frame[..5],
                ProductRecordForm::LegacyFeature
            ))
            .unwrap()
            .is_none()
        );
    }
}
