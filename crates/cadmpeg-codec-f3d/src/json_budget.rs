// SPDX-License-Identifier: Apache-2.0
//! Admission scan for JSON payloads decoded into typed records.

use std::cell::{Cell, RefCell};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};

struct CountJsonNodes<'a, 'b> {
    ctx: &'a DecodeContext<'b>,
    operation: &'static str,
    count: &'a Cell<u64>,
    overflowed: &'a Cell<bool>,
    refusal: &'a RefCell<Option<CodecError>>,
}

impl<'de> DeserializeSeed<'de> for CountJsonNodes<'_, '_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        let Some(next) = self.count.get().checked_add(1) else {
            self.overflowed.set(true);
            return Err(D::Error::custom("JSON node count overflows"));
        };
        self.count.set(next);
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for CountJsonNodes<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> { Ok(()) }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> { Ok(()) }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> { Ok(()) }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<(), E> { Ok(()) }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<(), E> { Ok(()) }
    fn visit_string<E: serde::de::Error>(self, _: String) -> Result<(), E> { Ok(()) }
    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> { Ok(()) }
    fn visit_none<E: serde::de::Error>(self) -> Result<(), E> { Ok(()) }

    fn visit_some<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        CountJsonNodes { ctx: self.ctx, operation: self.operation, count: self.count, overflowed: self.overflowed, refusal: self.refusal }
            .deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        let _depth = match self.ctx.enter_nested(self.operation) {
            Ok(depth) => depth,
            Err(error) => {
                self.refusal.replace(Some(error));
                return Err(A::Error::custom("JSON nesting limit exceeded"));
            }
        };
        while sequence.next_element_seed(CountJsonNodes {
            ctx: self.ctx, operation: self.operation, count: self.count, overflowed: self.overflowed, refusal: self.refusal,
        })?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let _depth = match self.ctx.enter_nested(self.operation) {
            Ok(depth) => depth,
            Err(error) => {
                self.refusal.replace(Some(error));
                return Err(A::Error::custom("JSON nesting limit exceeded"));
            }
        };
        while map.next_key_seed(CountJsonNodes {
            ctx: self.ctx, operation: self.operation, count: self.count, overflowed: self.overflowed, refusal: self.refusal,
        })?.is_some() {
            map.next_value_seed(CountJsonNodes {
                ctx: self.ctx, operation: self.operation, count: self.count, overflowed: self.overflowed, refusal: self.refusal,
            })?;
        }
        Ok(())
    }
}

/// Counts JSON values and object keys before a typed parse can allocate them.
/// Invalid JSON is left to the caller's parser so its existing error stays intact.
pub(crate) fn preflight(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    preflight_operation: &'static str,
    scan_operation: &'static str,
    collection_operation: &'static str,
) -> Result<bool, CodecError> {
    let payload_bytes = u64::try_from(payload.len())
        .map_err(|_| ctx.refuse_codec_limit(preflight_operation, 0, u64::MAX))?;
    ctx.charge_work(payload_bytes, scan_operation)?;
    let item_count = Cell::new(0_u64);
    let overflowed = Cell::new(false);
    let refusal = RefCell::new(None);
    let mut parser = serde_json::Deserializer::from_slice(payload);
    if (CountJsonNodes {
        ctx, operation: scan_operation, count: &item_count, overflowed: &overflowed, refusal: &refusal,
    })
        .deserialize(&mut parser)
        .and_then(|()| parser.end())
        .is_err()
    {
        if let Some(error) = refusal.into_inner() {
            return Err(error);
        }
        if overflowed.get() {
            return Err(ctx.refuse_codec_limit(preflight_operation, 0, u64::MAX));
        }
        return Ok(false);
    }
    ctx.charge_collection_items(item_count.get(), collection_operation)?;
    Ok(true)
}
