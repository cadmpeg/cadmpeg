// SPDX-License-Identifier: Apache-2.0
//! Nonempty Unicode attribute values and validated borrowed UTF-16 lanes.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct UnicodeValue(String);

impl UnicodeValue {
    pub(crate) fn new(value: String) -> Result<Self, &'static str> {
        if value.is_empty() {
            return Err("value must contain at least one Unicode scalar");
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for UnicodeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy)]
pub(super) struct UnicodeLane<'a>(&'a [u8]);

impl<'a> UnicodeLane<'a> {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        bytes: &'a [u8],
    ) -> Result<Option<Self>, CodecError> {
        let width = std::num::NonZeroUsize::new(2)
            .ok_or_else(|| CodecError::malformed("Unicode code-unit width must be nonzero"))?;
        let value = (|| {
            if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
                return None;
            }
            let mut high_surrogate = false;
            for bytes in propagate_resource!(ctx
                .admit_iter(bytes, "validate NX Unicode value lane")
                .map_err(CodecError::from))
            .chunks(width)
            {
                let unit = View::u16_be_at(bytes, 0)?;
                if high_surrogate {
                    (0xdc00..=0xdfff).contains(&unit).then_some(())?;
                    high_surrogate = false;
                } else if (0xd800..=0xdbff).contains(&unit) {
                    high_surrogate = true;
                } else {
                    (!(0xdc00..=0xdfff).contains(&unit)).then_some(())?;
                }
            }
            (!high_surrogate).then_some(Ok(Self(bytes)))
        })()
        .transpose()?;
        Ok(value)
    }

    pub(super) fn materialize(self, ctx: &DecodeContext<'_>) -> Result<UnicodeValue, CodecError> {
        let count = self.0.len() / 2;
        let width = std::num::NonZeroUsize::new(2)
            .ok_or_else(|| CodecError::malformed("Unicode code-unit width must be nonzero"))?;
        // Conversion scratch stays live through UTF-8 construction.
        let (mut code_units, _reservation) =
            ctx.temporary_vec(count, "NX Unicode conversion scratch")?;
        for bytes in ctx
            .admit_iter(self.0, "decode NX Unicode code units")?
            .chunks(width)
        {
            code_units.push(
                View::u16_be_at(bytes, 0)
                    .ok_or_else(|| CodecError::malformed("invalid admitted NX Unicode lane"))?,
            );
        }
        let mut length = 0usize;
        for scalar in char::decode_utf16(
            ctx.admit_iter(&code_units, "decode NX Unicode scalars")?
                .copied(),
        ) {
            let scalar =
                scalar.map_err(|_| CodecError::malformed("invalid admitted NX Unicode scalar"))?;
            length = length.checked_add(scalar.len_utf8()).ok_or_else(|| {
                ctx.refuse_codec_limit("NX Unicode UTF-8 payload", u64::MAX, u64::MAX)
            })?;
        }
        let mut value = ctx.retained_string(length, "NX Unicode UTF-8 payload")?;
        for scalar in char::decode_utf16(
            ctx.admit_iter(&code_units, "copy NX Unicode scalars")?
                .copied(),
        ) {
            ctx.push_retained_char(
                &mut value,
                scalar.map_err(|_| CodecError::malformed("invalid admitted NX Unicode scalar"))?,
                "NX Unicode UTF-8 payload",
            )?;
        }
        Ok(UnicodeValue(value))
    }
}

#[cfg(test)]
mod tests {
    use super::{UnicodeLane, UnicodeValue};

    #[test]
    fn unicode_character_copy_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        for (text, utf8_bytes) in [("μ", 2), ("🚀", 4)] {
            let bytes = text
                .encode_utf16()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>();
            let error = crate::test_support::resource_refusal_at(
                &bytes,
                ResourceDimension::WorkUnits,
                "NX Unicode UTF-8 payload",
                |ctx| UnicodeLane::new(ctx, &bytes)?.unwrap().materialize(ctx),
            );
            // Character copying counts its encoded UTF-8 bytes.
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "NX Unicode UTF-8 payload"
                    && limit.additional == utf8_bytes)
            );
            crate::test_support::with_decode_context(|ctx| {
                assert_eq!(
                    UnicodeLane::new(ctx, &bytes)
                        .unwrap()
                        .unwrap()
                        .materialize(ctx)
                        .unwrap()
                        .as_str(),
                    text
                );
            });
        }
    }

    #[test]
    fn unicode_values_preserve_scalars_and_reject_empty_or_incomplete_lanes() {
        crate::test_support::with_decode_context(|ctx| {
            let value = UnicodeValue::new("\0μ🚀".to_string()).unwrap();
            let wire = serde_json::to_string(value.as_str()).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), wire);
            assert_eq!(serde_json::from_str::<UnicodeValue>(&wire).unwrap(), value);
            assert!(serde_json::from_str::<UnicodeValue>("\"\"").is_err());
            let bytes = value
                .as_str()
                .encode_utf16()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>();
            assert_eq!(
                UnicodeLane::new(ctx, &bytes)
                    .unwrap()
                    .unwrap()
                    .materialize(ctx)
                    .unwrap(),
                value
            );
            for bytes in [
                &[][..],
                &[0][..],
                &[0xd8, 0][..],
                &[0xdc, 0][..],
                &[0xd8, 0, 0, 0][..],
            ] {
                assert!(UnicodeLane::new(ctx, bytes).unwrap().is_none());
            }
        });
    }
}
