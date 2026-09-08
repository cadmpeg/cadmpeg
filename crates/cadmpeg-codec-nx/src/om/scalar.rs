// SPDX-License-Identifier: Apache-2.0
//! Checked shifted scalar atoms and their decoded values.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShiftedBinary64([u8; 8]);

/// A checked scalar paired with its owning frame's offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocatedBinary64 {
    pub(crate) scalar: ShiftedBinary64,
    pub(crate) offset: usize,
}

impl ShiftedBinary64 {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        Self::try_from(<[u8; 8]>::try_from(bytes).ok()?).ok()
    }

    pub(crate) fn raw(self) -> [u8; 8] {
        self.0
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    pub(crate) fn value(self) -> f64 {
        let mut bytes = self.0;
        bytes[0] += 0x10;
        // endian-exception: reconstructed-scalar
        f64::from_be_bytes(bytes)
    }

    pub(crate) fn from_wire(value: f64, raw: [u8; 8]) -> Result<Self, &'static str> {
        let atom = Self::try_from(raw)?;
        if value.to_bits() != atom.value().to_bits() {
            return Err("scalars must match raw_scalars");
        }
        Ok(atom)
    }
}

impl TryFrom<[u8; 8]> for ShiftedBinary64 {
    type Error = &'static str;

    fn try_from(bytes: [u8; 8]) -> Result<Self, Self::Error> {
        if !is_shifted_ieee_f64_marker(bytes[0]) {
            return Err("raw_scalars must contain shifted binary64 markers");
        }
        // These marker intervals map only to finite binary64 exponent ranges.
        Ok(Self(bytes))
    }
}

pub(super) fn shifted_ieee_f64(bytes: &[u8]) -> Option<f64> {
    ShiftedBinary64::read(bytes).map(ShiftedBinary64::value)
}

pub(super) fn is_shifted_ieee_f64_marker(marker: u8) -> bool {
    matches!(marker, 0x20..=0x3f | 0xa0..=0xbf)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShiftedBinary32([u8; 4]);

impl ShiftedBinary32 {
    // Validate the value against the same borrowed raw byte window used by the reader.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(crate) fn from_wire(value: f64, raw: &[u8; 4]) -> Result<Self, &'static str> {
        let scalar = Self::read(raw).ok_or("raw_values must contain shifted binary32 atoms")?;
        if scalar.value().to_bits() != value.to_bits() {
            return Err("values must match raw_values atoms");
        }
        Ok(scalar)
    }

    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let raw = <[u8; 4]>::try_from(bytes).ok()?;
        matches!(raw[0], 0x40..=0x5f | 0xc0..=0xdf).then_some(Self(raw))
    }

    pub(crate) fn raw(self) -> [u8; 4] {
        self.0
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    pub(crate) fn value(self) -> f64 {
        let mut bytes = self.0;
        bytes[0] -= 0x10;
        // endian-exception: reconstructed-scalar
        f64::from(f32::from_be_bytes(bytes))
    }
}

/// One shifted binary32 or binary64 atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShiftedScalar {
    Binary32(ShiftedBinary32),
    Binary64(ShiftedBinary64),
}

impl ShiftedScalar {
    pub(crate) fn from_wire(value: f64, raw: &[u8]) -> Result<Self, &'static str> {
        let scalar = Self::read(raw).ok_or("raw_values must contain shifted scalar atoms")?;
        if scalar.raw().len() != raw.len() || scalar.value().to_bits() != value.to_bits() {
            return Err("values must match exact raw_values atoms");
        }
        Ok(scalar)
    }

    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        match PayloadScalarAtom::read(bytes)? {
            PayloadScalarAtom::Zero => None,
            PayloadScalarAtom::Binary32(atom) => Some(Self::Binary32(atom)),
            PayloadScalarAtom::Binary64(atom) => Some(Self::Binary64(atom)),
        }
    }

    pub(crate) fn value(self) -> f64 {
        match self {
            Self::Binary32(atom) => atom.value(),
            Self::Binary64(atom) => atom.value(),
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match self {
            Self::Binary32(atom) => atom.as_bytes(),
            Self::Binary64(atom) => atom.as_bytes(),
        }
    }
}

/// Wire spelling of a scalar atom's derived width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PayloadScalarEncoding {
    Zero,
    Binary32,
    Binary64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PayloadScalarAtom {
    Zero,
    Binary32(ShiftedBinary32),
    Binary64(ShiftedBinary64),
}

impl PayloadScalarAtom {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        match bytes.first()? {
            0 => Some(Self::Zero),
            0x40..=0x5f | 0xc0..=0xdf => ShiftedBinary32::read(bytes.get(..4)?).map(Self::Binary32),
            0x20..=0x3f | 0xa0..=0xbf => ShiftedBinary64::read(bytes.get(..8)?).map(Self::Binary64),
            _ => None,
        }
    }

    pub(crate) fn value(self) -> f64 {
        match self {
            Self::Zero => 0.0,
            Self::Binary32(atom) => atom.value(),
            Self::Binary64(atom) => atom.value(),
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match self {
            Self::Zero => &[0],
            Self::Binary32(atom) => &atom.0,
            Self::Binary64(atom) => &atom.0,
        }
    }

    pub(crate) fn encoding(self) -> PayloadScalarEncoding {
        match self {
            Self::Zero => PayloadScalarEncoding::Zero,
            Self::Binary32(_) => PayloadScalarEncoding::Binary32,
            Self::Binary64(_) => PayloadScalarEncoding::Binary64,
        }
    }

    pub(crate) fn from_wire(
        value: f64,
        encoding: PayloadScalarEncoding,
        raw: &[u8],
    ) -> Result<Self, &'static str> {
        let atom = Self::read(raw).ok_or("raw_values must contain complete scalar atoms")?;
        if atom.raw().len() != raw.len() {
            return Err("raw_values must contain exactly one scalar atom per entry");
        }
        if atom.encoding() != encoding {
            return Err("encodings must match raw_values");
        }
        if atom.value().to_bits() != value.to_bits() {
            return Err("values must match raw_values");
        }
        Ok(atom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_atoms_derive_width_and_reject_inconsistent_wire_data() {
        for (raw, expected) in [
            (vec![0], 0.0),
            (vec![0x50, 0x40, 0, 0], 3.0),
            (vec![0x2f, 0xf0, 0, 0, 0, 0, 0, 0], 1.0),
        ] {
            let atom = PayloadScalarAtom::read(&raw).unwrap();
            assert_eq!(atom.value(), expected);
            assert_eq!(atom.raw(), raw);
            assert_eq!(
                PayloadScalarAtom::from_wire(expected, atom.encoding(), &raw),
                Ok(atom)
            );
            assert!(PayloadScalarAtom::from_wire(expected + 1.0, atom.encoding(), &raw).is_err());
            let mut oversized = raw;
            oversized.push(0);
            assert!(PayloadScalarAtom::from_wire(expected, atom.encoding(), &oversized).is_err());
        }
        for marker in [0x40, 0x5f, 0xc0, 0xdf] {
            assert!(ShiftedBinary32::read(&[marker, 255, 255, 255])
                .unwrap()
                .value()
                .is_finite());
        }
        for bytes in [
            vec![],
            vec![1],
            vec![0x50, 0x40, 0],
            vec![0x2f, 0xf0, 0, 0, 0, 0, 0],
        ] {
            assert!(PayloadScalarAtom::read(&bytes).is_none());
        }
    }

    #[test]
    fn shifted_binary64_retains_bytes_and_derives_the_value() {
        let mut raw = 1.0_f64.to_be_bytes();
        raw[0] -= 0x10;
        let atom = ShiftedBinary64::try_from(raw).unwrap();
        assert_eq!(atom.value(), 1.0);
        assert_eq!(atom.raw(), raw);
        assert_eq!(ShiftedBinary64::from_wire(1.0, raw), Ok(atom));
        assert!(ShiftedBinary64::from_wire(2.0, raw).is_err());
        for marker in [0x20, 0x3f, 0xa0, 0xbf] {
            raw[0] = marker;
            assert!(ShiftedBinary64::try_from(raw).unwrap().value().is_finite());
        }
        for marker in [0x00, 0x1f, 0x40, 0x9f, 0xc0, 0xff] {
            raw[0] = marker;
            assert!(ShiftedBinary64::try_from(raw).is_err());
        }
        assert!(ShiftedBinary64::read(&[0; 7]).is_none());
    }
}

/// A scalar with the locations of its two identical source encodings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepeatedScalar<O> {
    pub(crate) scalar: ShiftedBinary64,
    pub(crate) witness_offsets: [O; 2],
}
