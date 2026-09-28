// SPDX-License-Identifier: Apache-2.0
//! Archive-wide wire primitives and checked numeric conversions.
#![deny(clippy::disallowed_methods)]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;

use cadmpeg_core::decode::{
    u64_from_index, BoundedCount, DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;

use crate::chunks::{checked_count_bytes, BoundedReader, FramingError};
use crate::curves::GeometryError;
use crate::layout::uuid_wire_form as uuid_wire;
use crate::settings::MillimeterScale;

/// A vector that must contain exactly a count proven against input.
#[derive(Debug)]
pub(crate) struct ExactVec<T> {
    values: Vec<T>,
    capacity: usize,
}

/// Charges and reserves a decoded collection before it grows.
pub(crate) fn reserve_collection<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(u64_from_index(additional), operation)?;
    values.try_reserve(additional).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::AllocationFailed,
            limit: u64::MAX,
            used: 0,
            additional: u64_from_index(additional),
            operation,
        })
    })
}

/// Charges and reserves entries before a decoded hash map grows.
pub(crate) fn reserve_hash_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(u64_from_index(additional), operation)?;
    values.try_reserve(additional).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::AllocationFailed,
            limit: u64::MAX,
            used: 0,
            additional: u64_from_index(additional),
            operation,
        })
    })
}

/// Charges and reserves entries before a decoded hash set grows.
pub(crate) fn reserve_hash_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(u64_from_index(additional), operation)?;
    values.try_reserve(additional).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::AllocationFailed,
            limit: u64::MAX,
            used: 0,
            additional: u64_from_index(additional),
            operation,
        })
    })
}

/// Creates a count-driven vector through the active decode session.
pub(crate) fn admitted_collection<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut values = Vec::new();
    reserve_collection(ctx, &mut values, count, operation)?;
    Ok(values)
}

/// Reserves a retained string after charging its known byte length.
pub(crate) fn admitted_retained_string(
    ctx: &DecodeContext<'_>,
    length: usize,
    operation: &'static str,
) -> Result<String, CodecError> {
    ctx.charge_retained(u64_from_index(length), operation)?;
    let mut value = String::new();
    value.try_reserve_exact(length).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::RetainedBytes,
            reason: ResourceFailure::AllocationFailed,
            limit: u64::MAX,
            used: 0,
            additional: u64_from_index(length),
            operation,
        })
    })?;
    Ok(value)
}

/// Copies UTF-8 text into session-retained storage.
pub(crate) fn copy_retained_string(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    String::from_utf8(ctx.copy_retained(value.as_bytes(), operation)?)
        .map_err(|error| CodecError::malformed(error.to_string()))
}

/// Formats retained text after measuring and charging its exact byte length.
pub(crate) fn admitted_format(
    ctx: &DecodeContext<'_>,
    arguments: fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    struct ByteCount(usize);

    impl fmt::Write for ByteCount {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            self.0 = self.0.checked_add(value.len()).ok_or(fmt::Error)?;
            Ok(())
        }
    }

    let mut bytes = ByteCount(0);
    fmt::write(&mut bytes, arguments)
        .map_err(|_| CodecError::malformed("retained text length overflow"))?;
    let mut text = admitted_retained_string(ctx, bytes.0, operation)?;
    fmt::write(&mut text, arguments)
        .map_err(|_| CodecError::malformed("retained text formatting failed"))?;
    Ok(text)
}

/// Admits both the formatted source text and the loss note's retained copy.
pub(crate) fn admitted_loss(
    ctx: &DecodeContext<'_>,
    code: crate::loss::RhinoLossCode,
    arguments: fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<cadmpeg_ir::report::loss::LossNote, CodecError> {
    let message = admitted_format(ctx, arguments, operation)?;
    ctx.charge_retained(u64_from_index(message.len()), operation)?;
    Ok(code.note(&message))
}

/// Serializes retained JSON after measuring and admitting its exact bytes.
pub(crate) fn admitted_json(
    ctx: &DecodeContext<'_>,
    value: &impl serde::Serialize,
    operation: &'static str,
) -> Result<String, CodecError> {
    struct ByteCount(usize);

    impl std::io::Write for ByteCount {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or(std::io::ErrorKind::OutOfMemory)?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut count = ByteCount(0);
    serde_json::to_writer(&mut count, value)
        .map_err(|error| CodecError::malformed(error.to_string()))?;
    ctx.charge_retained(u64_from_index(count.0), operation)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(count.0).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::RetainedBytes,
            reason: ResourceFailure::AllocationFailed,
            limit: u64::MAX,
            used: 0,
            additional: u64_from_index(count.0),
            operation,
        })
    })?;
    serde_json::to_writer(&mut bytes, value)
        .map_err(|error| CodecError::malformed(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| CodecError::malformed(error.to_string()))
}

/// Preserves the sorted object-key order of `serde_json::Value` without building
/// an uncharged intermediate tree.
pub(crate) fn admitted_canonical_json(
    ctx: &DecodeContext<'_>,
    value: &impl serde::Serialize,
    operation: &'static str,
) -> Result<String, CodecError> {
    struct ByteCount(usize);
    impl std::io::Write for ByteCount {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or(std::io::ErrorKind::OutOfMemory)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = ByteCount(0);
    serde_json::to_writer(&mut count, value)
        .map_err(|error| CodecError::malformed(error.to_string()))?;
    let _temporary = ctx.reserve_scoped(u64_from_index(count.0), operation)?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(count.0).map_err(|_| {
        allocation_failure(ResourceDimension::MaterializedBytes, count.0, operation)
    })?;
    serde_json::to_writer(&mut raw, value)
        .map_err(|error| CodecError::malformed(error.to_string()))?;
    let _tree = ctx.reserve_scoped(u64_from_index(count.0), operation)?;
    let failure = RefCell::new(None);
    let seed = CanonicalSeed {
        ctx,
        operation,
        failure: &failure,
    };
    let mut decoder = serde_json::Deserializer::from_slice(&raw);
    let canonical =
        serde::de::DeserializeSeed::deserialize(seed, &mut decoder).map_err(|error| {
            failure
                .into_inner()
                .unwrap_or_else(|| CodecError::malformed(error.to_string()))
        })?;
    decoder
        .end()
        .map_err(|error| CodecError::malformed(error.to_string()))?;
    admitted_json(ctx, &canonical, operation)
}

fn allocation_failure(
    dimension: ResourceDimension,
    amount: usize,
    operation: &'static str,
) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension,
        reason: ResourceFailure::AllocationFailed,
        limit: u64::MAX,
        used: 0,
        additional: u64_from_index(amount),
        operation,
    })
}

#[derive(Clone, Copy)]
struct CanonicalSeed<'a, 'b> {
    ctx: &'a DecodeContext<'a>,
    operation: &'static str,
    failure: &'b RefCell<Option<CodecError>>,
}

impl<'de> serde::de::DeserializeSeed<'de> for CanonicalSeed<'_, '_> {
    type Value = serde_json::Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        decoder.deserialize_any(CanonicalVisitor(self))
    }
}

struct CanonicalVisitor<'a, 'b>(CanonicalSeed<'a, 'b>);

impl<'de> serde::de::Visitor<'de> for CanonicalVisitor<'_, '_> {
    type Value = serde_json::Value;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }
    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(value.into())
    }
    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(value.into())
    }
    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(value.into())
    }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
        Ok(serde_json::Number::from_f64(value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number))
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }
    fn visit_some<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        serde::de::DeserializeSeed::deserialize(self.0, decoder)
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        let mut copy = String::new();
        copy.try_reserve_exact(value.len()).map_err(|_| {
            self.0.fail(allocation_failure(
                ResourceDimension::MaterializedBytes,
                value.len(),
                self.0.operation,
            ))
        })?;
        copy.push_str(value);
        Ok(serde_json::Value::String(copy))
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(serde_json::Value::String(value))
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(
        self,
        mut sequence: A,
    ) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(self.0)? {
            reserve_collection(self.0.ctx, &mut values, 1, self.0.operation)
                .map_err(|error| self.0.fail(error))?;
            values.push(value);
        }
        Ok(serde_json::Value::Array(values))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key_seed(CanonicalKeySeed(self.0))? {
            self.0
                .ctx
                .charge_collection_items(1, self.0.operation)
                .map_err(|error| self.0.fail(error))?;
            let value = map.next_value_seed(self.0)?;
            values.insert(key, value);
        }
        Ok(serde_json::Value::Object(values))
    }
}

struct CanonicalKeySeed<'a, 'b>(CanonicalSeed<'a, 'b>);

impl<'de> serde::de::DeserializeSeed<'de> for CanonicalKeySeed<'_, '_> {
    type Value = String;
    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        decoder.deserialize_string(CanonicalKeyVisitor(self.0))
    }
}

struct CanonicalKeyVisitor<'a, 'b>(CanonicalSeed<'a, 'b>);

impl serde::de::Visitor<'_> for CanonicalKeyVisitor<'_, '_> {
    type Value = String;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON object key")
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        let mut copy = String::new();
        copy.try_reserve_exact(value.len()).map_err(|_| {
            self.0.fail(allocation_failure(
                ResourceDimension::MaterializedBytes,
                value.len(),
                self.0.operation,
            ))
        })?;
        copy.push_str(value);
        Ok(copy)
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(value)
    }
}

impl CanonicalSeed<'_, '_> {
    fn fail<E: serde::de::Error>(&self, error: CodecError) -> E {
        *self.failure.borrow_mut() = Some(error);
        E::custom("JSON allocation refused")
    }
}

impl<T> ExactVec<T> {
    /// Charges and allocates storage for a count bounded by the input window.
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        count: BoundedCount,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let capacity = count.get();
        ctx.charge_collection_items(u64_from_index(capacity), operation)?;
        let mut values = Vec::new();
        values.try_reserve_exact(capacity).map_err(|_| {
            CodecError::ResourceLimit(ResourceLimit {
                dimension: ResourceDimension::CollectionItems,
                reason: ResourceFailure::AllocationFailed,
                limit: u64::MAX,
                used: 0,
                additional: u64_from_index(capacity),
                operation,
            })
        })?;
        Ok(Self { values, capacity })
    }

    /// Appends one value without exceeding the bounded count.
    pub(crate) fn push(&mut self, value: T) -> Result<(), CodecError> {
        if self.values.len() == self.capacity {
            return Err(CodecError::Malformed(
                "fixed-capacity vector overflow".to_owned(),
            ));
        }
        self.values.push(value);
        Ok(())
    }

    /// Returns the values if the bounded count was filled exactly.
    pub(crate) fn finish(self) -> Result<Vec<T>, CodecError> {
        if self.values.len() == self.capacity {
            Ok(self.values)
        } else {
            Err(CodecError::malformed(format_args!(
                "fixed-capacity vector contains {} of {} values",
                self.values.len(),
                self.capacity
            )))
        }
    }
}

/// A UUID in canonical textual byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Uuid {
    bytes: [u8; uuid_wire::LEN],
}

impl Uuid {
    /// Creates a UUID from bytes in canonical textual order.
    pub(crate) const fn from_canonical(bytes: [u8; uuid_wire::LEN]) -> Self {
        Self { bytes }
    }

    /// Parses the mixed-endian UUID wire representation.
    pub(crate) fn from_wire(bytes: [u8; uuid_wire::LEN]) -> Self {
        let mut canonical = [0; uuid_wire::LEN];
        for index in 0..4 {
            canonical[index] = bytes[3 - index];
        }
        for index in 0..2 {
            canonical[uuid_wire::DATA2 + index] = bytes[5 - index];
            canonical[uuid_wire::DATA3 + index] = bytes[7 - index];
        }
        canonical[uuid_wire::DATA4..].copy_from_slice(&bytes[uuid_wire::DATA4..]);
        Self { bytes: canonical }
    }

    /// Inverse of [`Uuid::from_wire`].
    #[cfg(test)]
    pub(crate) fn to_wire(self) -> [u8; uuid_wire::LEN] {
        let mut wire = [0; uuid_wire::LEN];
        for index in 0..4 {
            wire[3 - index] = self.bytes[index];
        }
        for index in 0..2 {
            wire[5 - index] = self.bytes[uuid_wire::DATA2 + index];
            wire[7 - index] = self.bytes[uuid_wire::DATA3 + index];
        }
        wire[uuid_wire::DATA4..].copy_from_slice(&self.bytes[uuid_wire::DATA4..]);
        wire
    }

    /// Returns the nil UUID.
    pub(crate) const fn nil() -> Self {
        Self {
            bytes: [0; uuid_wire::LEN],
        }
    }

    /// Returns whether this UUID is nil.
    pub(crate) fn is_nil(self) -> bool {
        self == Self::nil()
    }
}

const HEX_DIGITS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

struct UuidTail([u8; uuid_wire::LEN]);

impl fmt::Display for UuidTail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:x}", self.0[0] & 0x0f)?;
        for (index, byte) in self.0.iter().enumerate().skip(1) {
            if matches!(index, 4 | 6 | 8 | 10) {
                formatter.write_str("-")?;
            }
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let leading = HEX_DIGITS[usize::from(self.bytes[0] >> 4)];
        write!(formatter, "{leading}{}", UuidTail(self.bytes))
    }
}

/// Reads one mixed-endian UUID from the bounded reader.
pub(crate) fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

/// Reads one archive boolean stored as a 32-bit integer flag.
pub(crate) fn flag_i32(reader: &mut BoundedReader<'_>) -> Result<bool, FramingError> {
    Ok(reader.i32()? != 0)
}

/// Admits a finite `f64` and refuses a non-finite one at `offset`, the first
/// byte of the value.
pub(crate) fn finite(offset: usize, value: f64, label: &str) -> Result<FiniteReal, FramingError> {
    FiniteReal::new(value)
        .ok_or_else(|| FramingError::structural(offset, format!("{label} is not finite")))
}

/// Reads one `f64` and admits it finite, refusing a non-finite value at the
/// value's first byte.
pub(crate) fn read_finite(
    reader: &mut BoundedReader<'_>,
    label: &str,
) -> Result<FiniteReal, FramingError> {
    let offset = reader.position();
    let value = reader.f64()?;
    finite(offset, value, label)
}

/// Converts archive vector components to the model vector type.
pub(crate) fn vector(value: [f64; 3]) -> Vector3 {
    Vector3::new(value[0], value[1], value[2])
}

/// Multiplies an archive coordinate by a unit scale and admits the product
/// when it is finite.
///
/// The product is the only defect the caller can observe. A `MillimeterScale`
/// is finite and greater than zero, so a non-finite input value always makes a
/// non-finite product: `NaN` propagates and an infinity stays infinite.
pub(crate) fn scaled_coordinate(value: f64, scale: MillimeterScale) -> Option<FiniteReal> {
    FiniteReal::new(value * scale.value())
}

/// Multiplies the three archive coordinates of a point by a unit scale and
/// admits the product when every coordinate is finite.
pub(crate) fn scaled_point(value: [f64; 3], scale: MillimeterScale) -> Option<FinitePoint3> {
    FinitePoint3::new(Point3::new(
        value[0] * scale.value(),
        value[1] * scale.value(),
        value[2] * scale.value(),
    ))
}

/// Whether two vectors agree on every component within a tolerance.
pub(crate) fn close_vector(left: Vector3, right: Vector3, tolerance: f64) -> bool {
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

/// Reads a `width`-byte element count proven against the remaining window.
pub(crate) fn element_count(
    reader: &mut BoundedReader<'_>,
    width: usize,
) -> Result<usize, GeometryError> {
    let raw = reader.i32()?;
    let bytes = checked_count_bytes(
        raw,
        width,
        reader.remaining(),
        reader.remaining() / width,
        reader.position() - 4,
    )?;
    Ok(bytes / width)
}

#[cfg(test)]
mod tests;
