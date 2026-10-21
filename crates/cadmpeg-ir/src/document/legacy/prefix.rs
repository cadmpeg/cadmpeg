// SPDX-License-Identifier: Apache-2.0
//! Buffer one pre-version field while refusing duplicate nested keys.

use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

pub(super) struct PrefixValue;

impl<'de> DeserializeSeed<'de> for PrefixValue {
    type Value = Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for PrefixValue {
    type Value = Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a CADIR prefix field with distinct object keys")
    }

    fn visit_map<A: MapAccess<'de>>(self, access: A) -> Result<Value, A::Error> {
        cadmpeg_core::distinct_keys::json_object(serde::de::value::MapAccessDeserializer::new(
            access,
        ))
        .map(Value::Object)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = access.next_element_seed(Self)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_bool<E: Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_f64<E: Error>(self, value: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_string<E: Error>(self, value: String) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_unit<E: Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
}
