// SPDX-License-Identifier: Apache-2.0
//! Ordered EXTREFSTREAM handle tokens and their derived prefix fields.

use serde::{Deserialize, Serialize};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::convert::Infallible;

use crate::layout::extrefstream_handle_set_record;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "HandlesWire")]
pub(crate) struct ExtrefHandles(Vec<u32>);

#[derive(Serialize)]
struct HandlesRef<'a> {
    handles: &'a [u32],
    closing_duplicate: bool,
    prefix_byte_len: u64,
}

impl Serialize for ExtrefHandles {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        HandlesRef {
            handles: self.values(),
            closing_duplicate: self.closing_duplicate(),
            prefix_byte_len: u64::try_from(self.prefix_byte_len())
                .map_err(serde::ser::Error::custom)?,
        }
        .serialize(serializer)
    }
}

impl ExtrefHandles {
    pub(crate) fn new(tokens: Vec<u32>) -> Result<Self, &'static str> {
        match validate_tokens(&tokens, |tokens| Ok::<_, Infallible>(tokens.iter())) {
            Ok(validation) => validation.map(|()| Self(tokens)),
            Err(never) => match never {},
        }
    }

    pub(crate) fn from_wire(
        ctx: &DecodeContext<'_>,
        tokens: Vec<u32>,
    ) -> Result<Result<Self, &'static str>, CodecError> {
        let validation = validate_tokens(&tokens, |tokens| {
            ctx.admit_iter(tokens, "NX external reference handle ordering")
        })?;
        Ok(validation.map(|()| Self(tokens)))
    }

    pub(crate) fn serialized(&self) -> &[u32] {
        &self.0
    }

    pub(super) fn closing_duplicate(&self) -> bool {
        self.0.len() >= 2 && self.0[self.0.len() - 1] == self.0[self.0.len() - 2]
    }

    pub(super) fn values(&self) -> &[u32] {
        &self.0[..self.0.len() - usize::from(self.closing_duplicate())]
    }

    pub(crate) fn prefix_byte_len(&self) -> usize {
        extrefstream_handle_set_record::LEN + self.0.len() * 5 + 1
    }
}

fn validate_tokens<'tokens, E, I: Iterator<Item = &'tokens u32>>(
    tokens: &'tokens [u32],
    admit: impl FnOnce(&'tokens [u32]) -> Result<I, E>,
) -> Result<Result<(), &'static str>, E> {
    if !(1..=254).contains(&tokens.len()) {
        return Ok(Err("handles: encoded token count must be in 1..=254"));
    }
    let mut previous = None;
    if !admit(tokens)?.all(|token| {
        let ordered = match previous {
            Some(previous) => previous <= token,
            None => true,
        };
        previous = Some(token);
        ordered
    }) {
        return Ok(Err("handles: must be non-decreasing"));
    }
    Ok(Ok(()))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct HandlesWire {
    handles: Vec<u32>,
    closing_duplicate: bool,
    prefix_byte_len: u64,
}

#[cfg(test)]
std::thread_local! {
    static HANDLES_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<ExtrefHandles> for HandlesWire {
    fn from(value: ExtrefHandles) -> Self {
        HANDLES_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            handles: value.values().to_vec(),
            closing_duplicate: value.closing_duplicate(),
            prefix_byte_len: cadmpeg_core::decode::u64_from_index(value.prefix_byte_len()),
        }
    }
}

impl TryFrom<HandlesWire> for ExtrefHandles {
    type Error = &'static str;

    fn try_from(mut wire: HandlesWire) -> Result<Self, Self::Error> {
        if wire.closing_duplicate {
            let last = *wire
                .handles
                .last()
                .ok_or("closing_duplicate: requires a handle")?;
            wire.handles.push(last);
        }
        let value = Self::new(wire.handles)?;
        if value.closing_duplicate() != wire.closing_duplicate {
            return Err("closing_duplicate: must match the final encoded handle pair");
        }
        if cadmpeg_core::decode::u64_from_index(value.prefix_byte_len()) != wire.prefix_byte_len {
            return Err("prefix_byte_len: must match the encoded handle count");
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{ExtrefHandles, HANDLES_INTO_WIRE_COUNT};

    #[test]
    fn extref_handles_decode_admission_preserves_validation() {
        for tokens in [vec![], vec![7; 255], vec![9, 7], vec![7], vec![7, 7, 9], vec![7; 254]] {
            let decoded = crate::test_support::with_decode_context(|ctx| {
                ExtrefHandles::from_wire(ctx, tokens.clone()).unwrap()
            });
            assert_eq!(decoded, ExtrefHandles::new(tokens));
        }
    }

    #[test]
    fn extref_handles_ordering_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;

        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "NX external reference handle ordering",
            |ctx| ExtrefHandles::from_wire(ctx, vec![7, 7, 9]).map(|value| value.unwrap()),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "NX external reference handle ordering"));
    }

    #[test]
    fn wire_preserves_handle_occurrences_and_derived_fields() {
        for (tokens, handles, closing, length) in [
            (vec![7], vec![7], false, 31),
            (vec![7, 7], vec![7], true, 36),
            (vec![7, 7, 7], vec![7, 7], true, 41),
            (vec![7, 7, 9], vec![7, 7, 9], false, 41),
            (vec![7; 254], vec![7; 253], true, 1296),
        ] {
            let value = ExtrefHandles::new(tokens.clone()).unwrap();
            assert_eq!(value.serialized(), tokens);
            let wire = serde_json::json!({
                "handles": handles, "closing_duplicate": closing, "prefix_byte_len": length,
            });
            assert_eq!(serde_json::to_value(&value).unwrap(), wire);
            assert_eq!(
                serde_json::to_vec(&value).unwrap(),
                serde_json::to_vec(&super::HandlesWire::from(value.clone())).unwrap()
            );
            assert_eq!(
                serde_json::from_value::<ExtrefHandles>(wire).unwrap(),
                value
            );
        }
    }

    #[test]
    fn extref_handles_native_limit_refuses_before_owned_wire_conversion() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            #[serde(flatten)]
            handles: &'a ExtrefHandles,
        }

        let value = ExtrefHandles::new(vec![7, 7, 9]).unwrap();
        let record = Record {
            id: "nx:extref:handles#0",
            handles: &value,
        };
        let expected = serde_json::json!({
            "id": "nx:extref:handles#0",
            "handles": [7, 7, 9],
            "closing_duplicate": false,
            "prefix_byte_len": 41,
        });
        HANDLES_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(&record, expected);
        HANDLES_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn construction_rejects_inconsistent_prefixes() {
        for tokens in [vec![], vec![7; 255], vec![9, 7]] {
            assert!(ExtrefHandles::new(tokens).is_err());
        }
        for (handles, closing, length, field) in [
            (vec![], true, 26, "closing_duplicate"),
            (vec![7, 7], false, 36, "closing_duplicate"),
            (vec![7], false, 36, "prefix_byte_len"),
            (vec![7; 254], true, 1301, "handles"),
        ] {
            let error = serde_json::from_value::<ExtrefHandles>(serde_json::json!({
                "handles": handles, "closing_duplicate": closing, "prefix_byte_len": length,
            }))
            .unwrap_err();
            assert!(error.to_string().contains(field));
        }
    }
}
