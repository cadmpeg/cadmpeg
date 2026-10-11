// SPDX-License-Identifier: Apache-2.0
//! Exact typed comparison of the application serialization views.

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Deserializer};
use serde::de::{MapAccess, SeqAccess, Visitor};
use std::fmt;
use std::marker::PhantomData;

use super::{ApplicationPayloadWire, ApplicationPropertyWire, ApplicationRecordView, ApplicationRecordWire};
use crate::native::{LinkTarget, PropertyFamily};

// A struct read accepts a JSON object only. The core reader still owns all admission.
pub(super) struct ObjectOnly<T>(pub(super) T);

struct ObjectVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
    type Value = ObjectOnly<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an application wire object")
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        T::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(ObjectOnly)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ObjectOnly<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_struct("application wire object", &[], ObjectVisitor(PhantomData))
    }
}

struct ObjectVecVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVecVisitor<T> {
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of application wire objects")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut source: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(ObjectOnly(value)) = source.next_element::<ObjectOnly<T>>()? {
            values.push(value);
        }
        Ok(values)
    }
}

pub(super) fn object_vec<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Vec<T>, D::Error> {
    deserializer.deserialize_seq(ObjectVecVisitor(PhantomData))
}

pub(super) fn property_family<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PropertyFamily, D::Error> {
    let value = String::deserialize(deserializer)?;
    PropertyFamily::deserialize(serde::de::value::BorrowedStrDeserializer::new(&value))
}

const COMPARE: &str = "FreeCAD application field comparison";
const PAYLOAD: &str = "FreeCAD application payload comparison";

// Read the serialized link fields without the native link constructor's normalization.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApplicationLinkWire {
    #[serde(deserialize_with = "required_option")]
    document: Option<String>,
    #[serde(deserialize_with = "required_option")]
    document_attribute: Option<String>,
    #[serde(deserialize_with = "required_option")]
    object: Option<String>,
    subelements: Vec<String>,
}

fn required_option<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Option::deserialize(deserializer)
}

impl DecodeCost for PropertyFamily {
    const FIXED_BYTES: Option<u64> = Some(1);

    fn decode_cost(&self, _ctx: &DecodeContext<'_>, _operation: &'static str) -> Result<u64, CodecError> {
        Ok(1)
    }
}

impl ApplicationRecordWire {
    pub(super) fn matches(&self, ctx: &DecodeContext<'_>, expected: &ApplicationRecordView<'_>) -> Result<bool, CodecError> {
        if !ctx.equal(&self.id, &expected.id, COMPARE)? { return Ok(false); }
        if !ctx.equal(self.object.as_str(), expected.object, COMPARE)? { return Ok(false); }
        if !ctx.equal(self.type_name.as_str(), expected.type_name, COMPARE)? { return Ok(false); }
        if !ctx.equal(self.domain.as_str(), expected.domain, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.properties.len(), &expected.properties.len(), COMPARE)? { return Ok(false); }
        let mut pairs = self.properties.iter().zip(&expected.properties);
        while let Some((actual, expected)) = ctx.next_charged(&mut pairs, COMPARE)? {
            if !ctx.equal(actual.as_str(), *expected, COMPARE)? { return Ok(false); }
        }
        if !ctx.equal(self.dependencies.as_slice(), expected.dependencies, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.side_entries.len(), &expected.side_entries.len(), COMPARE)? { return Ok(false); }
        let mut pairs = self.side_entries.iter().zip(&expected.side_entries);
        while let Some((actual, expected)) = ctx.next_charged(&mut pairs, COMPARE)? {
            if !ctx.equal(actual.as_str(), *expected, COMPARE)? { return Ok(false); }
        }
        if !ctx.equal(&self.inert_payload, &expected.inert_payload, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.order, &expected.order, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.byte_start, &expected.byte_start, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.byte_end, &expected.byte_end, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.byte_len, &expected.byte_len, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.sha256, &expected.sha256, COMPARE)? { return Ok(false); }
        if !ctx.equal_bytes(&self.data, expected.data, PAYLOAD)? { return Ok(false); }
        if !ctx.equal(&self.property_records.len(), &expected.property_records.len(), COMPARE)? { return Ok(false); }
        let mut pairs = self.property_records.iter().zip(&expected.property_records);
        while let Some((actual, expected)) = ctx.next_charged(&mut pairs, COMPARE)? {
            if !actual.matches(ctx, expected)? { return Ok(false); }
        }
        Ok(true)
    }
}

impl ApplicationPropertyWire<String, Vec<u8>, Vec<Option<ObjectOnly<ApplicationLinkWire>>>> {
    fn matches(&self, ctx: &DecodeContext<'_>, expected: &ApplicationPropertyWire<&str, &[u8], &[Option<LinkTarget>]>) -> Result<bool, CodecError> {
        if !ctx.equal(&self.id, &expected.id, COMPARE)? { return Ok(false); }
        if !ctx.equal(self.object.as_str(), expected.object, COMPARE)? { return Ok(false); }
        if !ctx.equal(self.property.as_str(), expected.property, COMPARE)? { return Ok(false); }
        if !ctx.equal(self.type_name.as_str(), expected.type_name, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.family, &expected.family, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.order, &expected.order, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.links.len(), &expected.links.len(), COMPARE)? { return Ok(false); }
        let mut pairs = self.links.iter().zip(expected.links);
        while let Some((actual, expected)) = ctx.next_charged(&mut pairs, COMPARE)? {
            if !ctx.equal(&actual.is_some(), &expected.is_some(), COMPARE)? { return Ok(false); }
            if let (Some(ObjectOnly(actual)), Some(expected)) = (actual, expected) {
                if !ctx.equal(&actual.document.as_deref(), &expected.document_name(), COMPARE)?
                    || !ctx.equal(&actual.document_attribute.as_deref(), &expected.document_attribute(), COMPARE)?
                    || !ctx.equal(&actual.object.as_deref(), &Some(expected.object().unwrap_or("")), COMPARE)?
                    || !ctx.equal(actual.subelements.as_slice(), expected.subelements(), COMPARE)? {
                    return Ok(false);
                }
            }
        }
        if !ctx.equal(&self.byte_start, &expected.byte_start, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.byte_end, &expected.byte_end, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.byte_len, &expected.byte_len, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.sha256, &expected.sha256, COMPARE)? { return Ok(false); }
        if !ctx.equal_bytes(&self.data, expected.data, PAYLOAD)? { return Ok(false); }
        if !ctx.equal(&self.payloads.len(), &expected.payloads.len(), COMPARE)? { return Ok(false); }
        let mut pairs = self.payloads.iter().zip(&expected.payloads);
        while let Some((actual, expected)) = ctx.next_charged(&mut pairs, COMPARE)? {
            if !actual.matches(ctx, expected)? { return Ok(false); }
        }
        if !ctx.equal(&self.inert, &expected.inert, COMPARE)? { return Ok(false); }
        Ok(true)
    }
}

impl ApplicationPayloadWire<String, Vec<u8>> {
    fn matches(&self, ctx: &DecodeContext<'_>, expected: &ApplicationPayloadWire<&str, &[u8]>) -> Result<bool, CodecError> {
        if !ctx.equal(self.entry.as_str(), expected.entry, COMPARE)? { return Ok(false); }
        if !ctx.equal(self.name.as_str(), expected.name, COMPARE)? { return Ok(false); }
        if !ctx.equal(&self.byte_len, &expected.byte_len, COMPARE)? { return Ok(false); }
        if !ctx.equal(self.sha256.as_str(), expected.sha256, COMPARE)? { return Ok(false); }
        ctx.equal_bytes(&self.data, expected.data, PAYLOAD)
    }
}
