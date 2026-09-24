// SPDX-License-Identifier: Apache-2.0
//! Archive-wide wire primitives and checked numeric conversions.
#![deny(clippy::disallowed_methods)]

use std::fmt;

use cadmpeg_core::decode::BoundedCount;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};

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

impl<T> ExactVec<T> {
    /// Allocates storage for a count already bounded by the input window.
    pub(crate) fn new(count: BoundedCount) -> Result<Self, CodecError> {
        let capacity = count.get();
        let mut values = Vec::new();
        values
            .try_reserve_exact(capacity)
            .map_err(|_| CodecError::Io(std::io::Error::other("allocation failed")))?;
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

impl Uuid {
    /// Renders the UUID as a source identifier, which always has a leading hex digit.
    pub(crate) fn to_nonempty(self) -> cadmpeg_core::text::NonBlankString {
        cadmpeg_core::text::NonBlankString::prefixed(
            cadmpeg_core::text::NonWhitespaceChar::hex_digit(self.bytes[0] >> 4),
            UuidTail(self.bytes),
        )
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let leading = HEX_DIGITS[usize::from(self.bytes[0] >> 4)];
        write!(formatter, "{leading}{}", UuidTail(self.bytes))
    }
}

/// Render comma-separated native property values in their stored order.
pub(crate) fn comma_list<T: ToString>(values: impl IntoIterator<Item = T>) -> String {
    values
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Reads one mixed-endian UUID from the bounded reader.
pub(crate) fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

/// Reads one archive boolean stored as a 32-bit integer flag.
pub(crate) fn flag_i32(reader: &mut BoundedReader<'_>) -> Result<bool, FramingError> {
    Ok(reader.i32()? != 0)
}

/// Refuses a non-finite `f64` at `offset`, the first byte of the value.
pub(crate) fn finite(offset: usize, value: f64, label: &str) -> Result<f64, FramingError> {
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| FramingError::structural(offset, format!("{label} is not finite")))
}

/// Reads one `f64` and refuses a non-finite value at the value's first byte.
pub(crate) fn read_finite(
    reader: &mut BoundedReader<'_>,
    label: &str,
) -> Result<f64, FramingError> {
    let offset = reader.position();
    let value = reader.f64()?;
    finite(offset, value, label)
}

/// Converts archive vector components to the model vector type.
pub(crate) fn vector(value: [f64; 3]) -> Vector3 {
    Vector3::new(value[0], value[1], value[2])
}

/// Multiplies an archive coordinate by a unit scale.
///
/// The product is the only defect the caller can observe. A `MillimeterScale`
/// is finite and greater than zero, so a non-finite input value always makes a
/// non-finite product: `NaN` propagates and an infinity stays infinite.
pub(crate) fn scaled_coordinate(value: f64, scale: MillimeterScale) -> Option<f64> {
    let result = value * scale.value();
    result.is_finite().then_some(result)
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
