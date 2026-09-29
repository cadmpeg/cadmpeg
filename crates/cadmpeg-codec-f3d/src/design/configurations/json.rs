// SPDX-License-Identifier: Apache-2.0
//! Build configuration JSON values under the caller decode budget.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;

pub(super) struct ConfigurationTextSeed<'a, 'b> {
    pub(super) ctx: Option<&'a DecodeContext<'b>>,
    pub(super) refusal: &'a mut Option<CodecError>,
    pub(super) operation: &'static str,
    pub(super) entry: bool,
    pub(super) expected: &'static str,
}

impl<'de> DeserializeSeed<'de> for ConfigurationTextSeed<'_, '_> {
    type Value = String;
    fn deserialize<D>(self, deserializer: D) -> Result<String, D::Error>
    where D: serde::Deserializer<'de> {
        if self.entry {
            if let Some(ctx) = self.ctx {
                if let Err(error) = ctx.charge_collection_items(1, "f3d configuration JSON object entry") {
                    *self.refusal = Some(error);
                    return Err(serde::de::Error::custom("configuration JSON resource limit"));
                }
            }
        }
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for ConfigurationTextSeed<'_, '_> {
    type Value = String;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expected)
    }
    fn visit_str<E>(self, text: &str) -> Result<String, E>
    where E: serde::de::Error {
        match super::copy_configuration_text(self.ctx, text, self.operation) {
            Ok(text) => Ok(text),
            Err(error) => {
                *self.refusal = Some(error);
                Err(serde::de::Error::custom("configuration JSON resource limit"))
            }
        }
    }
}

pub(super) struct ConfigurationFieldSeed;

impl<'de> DeserializeSeed<'de> for ConfigurationFieldSeed {
    type Value = bool;
    fn deserialize<D>(self, deserializer: D) -> Result<bool, D::Error>
    where D: serde::Deserializer<'de> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for ConfigurationFieldSeed {
    type Value = bool;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string")
    }
    fn visit_str<E>(self, field: &str) -> Result<bool, E> where E: serde::de::Error {
        Ok(field == "configurations")
    }
}

struct ConfigurationValueSeed<'a, 'b> {
    ctx: &'a DecodeContext<'b>,
    refusal: &'a mut Option<CodecError>,
    member: bool,
}

impl ConfigurationValueSeed<'_, '_> {
    fn admit<T, E: serde::de::Error>(&mut self, result: Result<T, CodecError>) -> Result<T, E> {
        result.map_err(|error| {
            *self.refusal = Some(error);
            E::custom("configuration JSON resource limit")
        })
    }
}

impl<'de> DeserializeSeed<'de> for ConfigurationValueSeed<'_, '_> {
    type Value = Value;
    fn deserialize<D>(mut self, deserializer: D) -> Result<Value, D::Error>
    where D: serde::Deserializer<'de> {
        let ctx = self.ctx;
        let _depth = self.admit(ctx.enter_nested("f3d configuration JSON depth"))?;
        self.admit(ctx.charge_work(1, "f3d configuration JSON value work"))?;
        if self.member {
            self.admit(ctx.charge_collection_items(1, "f3d configuration JSON array member"))?;
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ConfigurationValueSeed<'_, '_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any valid JSON value")
    }
    fn visit_bool<E>(self, value: bool) -> Result<Value, E> { Ok(Value::Bool(value)) }
    fn visit_i64<E>(self, value: i64) -> Result<Value, E> { Ok(Value::Number(value.into())) }
    fn visit_u64<E>(self, value: u64) -> Result<Value, E> { Ok(Value::Number(value.into())) }
    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number))
    }
    fn visit_unit<E>(self) -> Result<Value, E> { Ok(Value::Null) }
    fn visit_str<E>(mut self, text: &str) -> Result<Value, E> where E: serde::de::Error {
        let copied = super::copy_configuration_text(Some(self.ctx), text,
            "f3d configuration JSON text");
        self.admit(copied).map(Value::String)
    }
    fn visit_seq<S>(mut self, mut sequence: S) -> Result<Value, S::Error>
    where S: SeqAccess<'de> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(ConfigurationValueSeed {
            ctx: self.ctx, refusal: &mut *self.refusal, member: true,
        })? {
            self.admit(values.try_reserve(1).map_err(|_| {
                self.ctx.refuse_codec_limit("f3d configuration JSON array allocation", 0, 1)
            }))?;
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<M>(mut self, mut map: M) -> Result<Value, M::Error>
    where M: MapAccess<'de> {
        let mut fields = serde_json::Map::new();
        while let Some(key) = map.next_key_seed(ConfigurationTextSeed {
            ctx: Some(self.ctx), refusal: &mut *self.refusal,
            operation: "f3d configuration JSON key", entry: true, expected: "a string key",
        })? {
            // serde_json's raw-value feature interprets this first-key carrier.
            if fields.is_empty() && key == "$serde_json::private::RawValue" {
                let raw = map.next_value_seed(ConfigurationTextSeed {
                    ctx: Some(self.ctx), refusal: &mut *self.refusal,
                    operation: "f3d configuration raw JSON text", entry: false, expected: "raw value",
                })?;
                let raw_len = u64::try_from(raw.len()).map_err(|_| {
                    self.ctx.refuse_codec_limit("f3d configuration raw JSON scratch", 0, 1)
                });
                let raw_len = self.admit(raw_len)?;
                let _raw_scratch = self.admit(self.ctx.reserve_scoped(raw_len,
                    "f3d configuration raw JSON scratch"))?;
                let mut deserializer = serde_json::Deserializer::from_str(&raw);
                let value = ConfigurationValueSeed {
                    ctx: self.ctx, refusal: &mut *self.refusal, member: false,
                }.deserialize(&mut deserializer).map_err(serde::de::Error::custom)?;
                deserializer.end().map_err(serde::de::Error::custom)?;
                return Ok(value);
            }
            let value = map.next_value_seed(ConfigurationValueSeed {
                ctx: self.ctx, refusal: &mut *self.refusal, member: false,
            })?;
            // discarded-value: duplicate JSON object keys retain the last value.
            let _ = fields.insert(key, value);
        }
        Ok(Value::Object(fields))
    }
}

pub(super) fn parse_configuration_payload(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
    bytes: &[u8],
) -> Result<Value, CodecError> {
    let _scratch = ctx.reserve_scoped(u64::try_from(bytes.len())
        .map_err(|_| ctx.refuse_codec_limit("f3d configuration JSON", 0, 1))?,
        "f3d configuration JSON")?;
    let mut refusal = None;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let parsed = ConfigurationValueSeed { ctx, refusal: &mut refusal, member: false }
        .deserialize(&mut deserializer).and_then(|value| {
            deserializer.end()?;
            Ok(value)
        });
    match parsed {
        Ok(value) => Ok(value),
        Err(error) => match refusal {
            Some(error) => Err(error),
            None => Err(CodecError::Malformed(crate::design::text::format_design_text(
                Some(ctx), format_args!("invalid F3D configuration JSON {entry_name}: {error}"),
                "f3d configuration JSON diagnostic")?)),
        },
    }
}

#[cfg(test)]
mod tests;
