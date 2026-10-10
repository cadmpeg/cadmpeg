// SPDX-License-Identifier: Apache-2.0
//! Replay stored JSON text into a serializer.
//!
//! [`emit`] drives a serializer from the parse, reading one container at a
//! time. Each container is a parse of its own, so the parser's own recursion
//! limit never accumulates; the descent is counted here instead, against the
//! one bound every native record field answers to.

use std::fmt;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};

use serde::{de, ser};
use serde_json::value::RawValue;

use super::nests_too_deep_message;

/// Emit the JSON value held in `json` into `serializer`, entering at most
/// `depth` further containers.
///
/// The counter bounds this function's own recursion, whatever serializer it
/// drives. A serializer that counts as well is handed the same remainder, so
/// the two refuse at the same container. The caller admits the root text pass;
/// each child parse admits its own pass when its serializer is called.
pub(super) fn emit<S: ser::Serializer>(
    json: &str,
    serializer: S,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<S::Ok, S::Error> {
    let mut scratch = ctx.reserve_scoped(0, "serialize native record").map_err(ser::Error::custom)?;
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let emitted = de::Deserializer::deserialize_any(
        &mut deserializer,
        Emit {
            serializer,
            depth,
            ctx,
            json,
            scratch: &mut scratch,
        },
    )
    .map_err(de_to_ser)?;
    deserializer.end().map_err(de_to_ser)?;
    Ok(emitted)
}

/// The refusal a container past the bound reports.
fn nests_too_deep<E: de::Error>() -> E {
    de::Error::custom(nests_too_deep_message())
}

fn ser_to_de<S: ser::Error, D: de::Error>(error: S) -> D {
    de::Error::custom(error)
}

fn de_to_ser<D: de::Error, S: ser::Error>(error: D) -> S {
    ser::Error::custom(error)
}

/// Writes whatever it is handed straight into a serializer.
struct Emit<'a, 'scratch, S> {
    /// Where the value is written.
    serializer: S,
    /// Containers this value may still enter.
    depth: usize,
    ctx: &'a DecodeContext<'a>,
    json: &'a str,
    scratch: &'scratch mut ScopedReservation<'a>,
}

impl<'de, S: ser::Serializer> de::Visitor<'de> for Emit<'_, '_, S> {
    type Value = S::Ok;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.serializer.serialize_unit().map_err(ser_to_de)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        self.serializer.serialize_bool(value).map_err(ser_to_de)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        self.serializer.serialize_i64(value).map_err(ser_to_de)
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        self.serializer.serialize_u64(value).map_err(ser_to_de)
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<Self::Value, E> {
        self.serializer.serialize_i128(value).map_err(ser_to_de)
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<Self::Value, E> {
        self.serializer.serialize_u128(value).map_err(ser_to_de)
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        self.serializer.serialize_f64(value).map_err(ser_to_de)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.serializer.serialize_str(value).map_err(ser_to_de)
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let Some(depth) = self.depth.checked_sub(1) else {
            return Err(nests_too_deep());
        };
        let mut sequence = self
            .serializer
            .serialize_seq(access.size_hint())
            .map_err(ser_to_de)?;
        while let Some(value) = access.next_element::<&RawValue>()? {
            let child = Replay {
                json: value.get(),
                depth,
                ctx: self.ctx,
            };
            ser::SerializeSeq::serialize_element(&mut sequence, &child).map_err(ser_to_de)?;
        }
        ser::SerializeSeq::end(sequence).map_err(ser_to_de)
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let Some(depth) = self.depth.checked_sub(1) else {
            return Err(nests_too_deep());
        };
        let mut map = self
            .serializer
            .serialize_map(access.size_hint())
            .map_err(ser_to_de)?;
        let bytes = self.json.as_bytes();
        let mut offset = 0;
        while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
            self.ctx.charge_work(1, "construct canonical native value")
                .map_err(de::Error::custom)?;
            offset += 1;
        }
        offset += 1; // The opening brace was consumed before visit_map.
        let mut scratch_bytes = 0;
        loop {
            while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
                self.ctx.charge_work(1, "construct canonical native value")
                    .map_err(de::Error::custom)?;
                offset += 1;
            }
            if bytes.get(offset) == Some(&b',') {
                offset += 1;
                while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
                    self.ctx.charge_work(1, "construct canonical native value")
                        .map_err(de::Error::custom)?;
                    offset += 1;
                }
            }
            // Bound this key before serde decodes into its reusable scratch.
            // The container parser preserves key-error positions.
            if bytes.get(offset) == Some(&b'"') {
                let start = offset;
                self.ctx.charge_work(1, "construct canonical native value")
                    .map_err(de::Error::custom)?;
                offset += 1;
                let mut escaped = false;
                let mut has_escape = false;
                while let Some(&byte) = bytes.get(offset) {
                    self.ctx.charge_work(1, "construct canonical native value")
                        .map_err(de::Error::custom)?;
                    offset += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                        has_escape = true;
                    } else if byte == b'"' {
                        break;
                    }
                }
                if has_escape {
                    // One raw-key length bounds the old allocation; two bound
                    // the growing allocation. Both can overlap during growth.
                    let needed = u64_from_index(offset - start).checked_mul(3)
                        .ok_or_else(|| de::Error::custom(self.ctx.refuse_codec_limit(
                            "serialize native record", u64::MAX - 1, u64::MAX,
                        )))?.max(8);
                    if needed > scratch_bytes {
                        self.scratch.grow(needed - scratch_bytes).map_err(de::Error::custom)?;
                        scratch_bytes = needed;
                    }
                }
            }
            let Some(()) = access.next_key_seed(Key { map: &mut map })? else {
                break;
            };
            let value = access.next_value::<&RawValue>()?;
            // RawValue borrows this parser's source, so the end of the read
            // member is the next key's delimiter and whitespace boundary.
            offset = (value.get().as_ptr() as usize)
                .checked_sub(self.json.as_ptr() as usize)
                .and_then(|offset| offset.checked_add(value.get().len()))
                .ok_or_else(|| de::Error::custom(self.ctx.refuse_codec_limit(
                    "construct canonical native value", u64::MAX - 1, u64::MAX,
                )))?;
            let child = Replay {
                json: value.get(),
                depth,
                ctx: self.ctx,
            };
            ser::SerializeMap::serialize_value(&mut map, &child).map_err(ser_to_de)?;
        }
        ser::SerializeMap::end(map).map_err(ser_to_de)
    }
}

/// A repeatable borrowed JSON value handed to an arbitrary serializer.
///
/// A serializer may read a value more than once, or skip it. Consume the raw
/// child before handing it over, then start a fresh parse for each read.
/// Each parser reads only one container; `RawValue` scans its children without
/// the parser recursion limit. No parsed child tree is retained here, and the
/// child carries what is left of the value's container budget, so a repeat
/// read restarts from the same depth the first read used.
struct Replay<'a> {
    /// This child's source text.
    json: &'a str,
    /// Containers this child may still enter.
    depth: usize,
    ctx: &'a DecodeContext<'a>,
}

impl ser::Serialize for Replay<'_> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.ctx.charge_work(u64_from_index(self.json.len()), "construct canonical native value")
            .map_err(ser::Error::custom)?;
        emit(self.json, serializer, self.depth, self.ctx)
    }
}

/// Serializes a decoded object key while serde's borrowed or scratch view is live.
struct Key<'a, M> {
    map: &'a mut M,
}

impl<'de, M: ser::SerializeMap> de::DeserializeSeed<'de> for Key<'_, M> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de, M: ser::SerializeMap> de::Visitor<'de> for Key<'_, M> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON object key")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.map.serialize_key(value).map_err(ser_to_de)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::emit;
    use crate::native::MAX_NATIVE_NESTING_DEPTH;
    use serde_json::Value;

    #[test]
    fn raw_replay_charges_only_text_passes_that_run() {
        use super::u64_from_index;
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        for (json, work) in [("7", 1), ("[[7]]", 5 + 3 + 1)] {
            let run = |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                ctx.charge_work(
                    u64_from_index(json.len()),
                    "construct canonical native value",
                )?;
                let result = emit(
                    json,
                    serde_json::value::Serializer,
                    MAX_NATIVE_NESTING_DEPTH,
                    &ctx,
                );
                ctx.finish_session()?;
                result.map_err(|error| CodecError::malformed(error.to_string()))
            };
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                "construct canonical native value",
                run,
            );
            // One root text pass, then each read child: 1 byte, or 5 + 3 + 1 bytes.
            assert_eq!(
                run(work).unwrap(),
                serde_json::from_str::<Value>(json).unwrap()
            );
        }
    }

    /// Replay one whole native record field.
    fn emit_field<S: serde::ser::Serializer>(json: &str, serializer: S) -> Result<S::Ok, S::Error> {
        let ctx = cadmpeg_test_support::service_decode_context();
        emit(json, serializer, MAX_NATIVE_NESTING_DEPTH, &ctx)
    }

    const TEXT: &str = concat!(
        r#"{"id":"pin#0","a":[null,true,false,-1,0,1.5,2.0,1e-7,10000000000.0],"#,
        r#""b":{"":[],"c\td":{},"nested":{"a":[[[1]]]}},"#,
        r#""é key":"quote\" back\\ tab\t bell\u0007 é","z":18446744073709551615}"#
    );

    #[test]
    fn emits_the_value_a_parse_would_produce() {
        let replayed = emit_field(TEXT, serde_json::value::Serializer).unwrap();
        assert_eq!(replayed, serde_json::from_str::<Value>(TEXT).unwrap());
    }

    #[test]
    fn compact_emission_reproduces_the_source_text() {
        let mut out = Vec::new();
        emit_field(TEXT, &mut serde_json::Serializer::new(&mut out)).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), TEXT);
    }

    #[test]
    fn a_borrowed_member_can_be_serialized_more_than_once() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let member = super::Replay {
            json: TEXT,
            depth: MAX_NATIVE_NESTING_DEPTH,
            ctx: &ctx,
        };
        let expected = serde_json::from_str::<Value>(TEXT).unwrap();
        assert_eq!(serde_json::to_value(&member).unwrap(), expected);
        assert_eq!(serde_json::to_string(&member).unwrap(), TEXT);
        assert_eq!(serde_json::to_value(&member).unwrap(), expected);
    }

    /// One budget spans the whole replay: the parser's own limit restarts per
    /// container, so without this counter the descent follows the input text.
    #[test]
    fn one_budget_spans_the_whole_replay() {
        let chain =
            |containers: usize| format!("{}7{}", "[".repeat(containers), "]".repeat(containers));

        let admitted = chain(MAX_NATIVE_NESTING_DEPTH);
        let mut out = Vec::new();
        emit_field(&admitted, &mut serde_json::Serializer::new(&mut out)).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), admitted);

        let error = emit_field(
            &chain(MAX_NATIVE_NESTING_DEPTH + 1),
            serde_json::value::Serializer,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains(&format!(
                "native value nests deeper than {MAX_NATIVE_NESTING_DEPTH} containers"
            )),
            "{error}"
        );
    }

    #[test]
    fn raw_replay_escaped_key_scratch_is_scoped_and_has_no_owned_copy() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let json = r#"{"a\u0062":7,"c\td":8}"#;
        let key_bytes = r#""a\u0062""#.len();
        // Old and growing scratch allocations overlap, with an eight-byte minimum.
        let scratch_bytes = super::u64_from_index(key_bytes * 3).max(8);
        let run = |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            ctx.charge_work(super::u64_from_index(json.len()), "construct canonical native value")?;
            let emitted = emit(json, serde_json::value::Serializer, MAX_NATIVE_NESTING_DEPTH, &ctx);
            ctx.charge_work(0, "construct canonical native value")?;
            let emitted = emitted.map_err(|error| CodecError::malformed(error.to_string()))?;
            assert_eq!(emitted, serde_json::json!({"ab":7,"c\td":8}));
            let released = ctx.reserve_scoped(cap, "released raw key scratch")?;
            drop(released);
            ctx.finish_session()?;
            Ok(emitted)
        };
        let CodecError::ResourceLimit(limit) = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "serialize native record",
            run,
        ) else {
            panic!("escaped key scratch refusal");
        };
        assert_eq!(limit.additional, scratch_bytes);
        assert_eq!(limit.used, 0);
        run(scratch_bytes).unwrap();
    }

    #[test]
    fn raw_replay_borrowed_keys_need_no_scratch_or_owned_copy() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let json = r#"{"é key":7,"":8}"#;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        ctx.charge_work(super::u64_from_index(json.len()), "construct canonical native value").unwrap();
        assert_eq!(
            emit(json, serde_json::value::Serializer, MAX_NATIVE_NESTING_DEPTH, &ctx).unwrap(),
            serde_json::json!({"é key":7,"":8})
        );
        ctx.finish_session().unwrap();
    }

    #[test]
    fn raw_replay_long_escaped_key_scratch_admits_reallocation_overlap() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let prefix = "a".repeat(1024);
        let key = format!("{prefix}\t");
        let raw_key = format!(r#""{prefix}\t""#);
        let json = format!("{{{raw_key}:7}}");
        // The 1024-byte prefix fills the first scratch allocation; decoding
        // the final escape can double it while the old allocation is live.
        let scratch_bytes = super::u64_from_index(3 * raw_key.len());
        let run = |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            ctx.charge_work(super::u64_from_index(json.len()), "construct canonical native value")?;
            let value = emit(&json, serde_json::value::Serializer, MAX_NATIVE_NESTING_DEPTH, &ctx);
            ctx.charge_work(0, "construct canonical native value")?;
            let value = value.map_err(|error| CodecError::malformed(error.to_string()))?;
            assert_eq!(value.get(&key), Some(&Value::from(7)));
            let released = ctx.reserve_scoped(cap, "released long raw key scratch")?;
            drop(released);
            ctx.finish_session()?;
            Ok(value)
        };
        let CodecError::ResourceLimit(limit) = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "serialize native record",
            run,
        ) else {
            panic!("key growth must be admitted");
        };
        assert_eq!(limit.additional, scratch_bytes);
        assert_eq!(limit.used, 0);
        run(scratch_bytes).unwrap();
    }

    #[test]
    fn raw_replay_each_repeated_child_read_is_admitted() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1 + 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let child = super::Replay { json: "7", depth: MAX_NATIVE_NESTING_DEPTH, ctx: &ctx };
        assert_eq!(serde_json::to_value(&child).unwrap(), Value::from(7));
        assert_eq!(serde_json::to_value(&child).unwrap(), Value::from(7));
        assert!(serde_json::to_value(&child).is_err());
        let limit = ctx.resource_refusal().unwrap();
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "construct canonical native value");
        assert_eq!(limit.used, 2); // Two one-byte scalar reads have run.
        assert_eq!(limit.additional, 1); // A third one-byte scalar read is refused.
        assert!(ctx.finish_session().is_err());
    }

    #[test]
    fn raw_replay_key_refusals_preserve_parser_messages() {
        for json in [
            r#"{"\uD800":0}"#,
            r#"{"\uDC00":0}"#,
            "{\n\"ok\":0,\n\"\\uD800\":1\n}",
        ] {
            let expected = serde_json::from_str::<Value>(json).unwrap_err().to_string();
            let error = emit_field(json, serde_json::value::Serializer).unwrap_err();
            assert_eq!(error.to_string(), expected);
        }
    }
}
