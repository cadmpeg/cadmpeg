// SPDX-License-Identifier: Apache-2.0
//! Archive-wide wire primitives and checked numeric conversions.
#![deny(clippy::disallowed_methods)]

use std::fmt;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::BoundedCount;

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
            Err(CodecError::Malformed(format!(
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
    bytes: [u8; 16],
}

impl Uuid {
    /// Creates a UUID from bytes in canonical textual order.
    pub(crate) const fn from_canonical(bytes: [u8; 16]) -> Self {
        Self { bytes }
    }

    /// Parses the mixed-endian UUID wire representation.
    pub(crate) fn from_wire(bytes: [u8; 16]) -> Self {
        let mut canonical = [0; 16];
        for index in 0..4 {
            canonical[index] = bytes[3 - index];
        }
        for index in 0..2 {
            canonical[4 + index] = bytes[5 - index];
            canonical[6 + index] = bytes[7 - index];
        }
        canonical[8..].copy_from_slice(&bytes[8..]);
        Self { bytes: canonical }
    }

    /// Returns the nil UUID.
    pub(crate) const fn nil() -> Self {
        Self { bytes: [0; 16] }
    }

    /// Returns whether this UUID is nil.
    pub(crate) fn is_nil(self) -> bool {
        self == Self::nil()
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.bytes.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                formatter.write_str("-")?;
            }
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Multiplies an archive coordinate by a positive finite unit scale.
pub(crate) fn scaled_coordinate(value: f64, scale: f64) -> Option<f64> {
    if !value.is_finite() || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let result = value * scale;
    result.is_finite().then_some(result)
}
