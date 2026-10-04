// SPDX-License-Identifier: Apache-2.0
//! Signed Q1.55 scalars and their exact atom markers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Q155 {
    raw: [u8; 7],
    value_bits: u64,
}

impl Q155 {
    pub(super) fn from_raw(raw: [u8; 7]) -> Option<Self> {
        let unsigned = raw
            .into_iter()
            .fold(0_u64, |value, byte| (value << 8) | u64::from(byte));
        let signed = i64::try_from(unsigned).ok()?;
        let signed = if unsigned & (1_u64 << 55) == 0 {
            signed
        } else {
            signed - (1_i64 << 56)
        };
        let value = cadmpeg_core::convert::f64_from_i64(signed)? / 36_028_797_018_963_968.0;
        Some(Self {
            raw,
            value_bits: value.to_bits(),
        })
    }

    pub(crate) fn raw(self) -> [u8; 7] {
        self.raw
    }

    pub(crate) fn value(self) -> f64 {
        f64::from_bits(self.value_bits)
    }

    pub(crate) fn from_wire(value: f64, raw: [u8; 7]) -> Result<Self, &'static str> {
        let scalar =
            Self::from_raw(raw).ok_or("signed Q1.55 raw_values must be exactly representable")?;
        if scalar.value().to_bits() != value.to_bits() {
            return Err("values must match signed Q1.55 raw_values");
        }
        Ok(scalar)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Q155Marker {
    M30,
    MB0,
}

impl Q155Marker {
    pub(crate) fn read(byte: u8) -> Option<Self> {
        match byte {
            0x30 => Some(Self::M30),
            0xb0 => Some(Self::MB0),
            _ => None,
        }
    }

    pub(crate) fn byte(self) -> u8 {
        match self {
            Self::M30 => 0x30,
            Self::MB0 => 0xb0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Q155Atom {
    pub(crate) marker: Q155Marker,
    pub(crate) scalar: Q155,
}

pub(crate) mod pair_wire {
    use super::Q155;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        values: [f64; 2],
        raw_values: [[u8; 7]; 2],
    }

    pub(crate) fn serialize<S: Serializer>(
        values: &[Q155; 2],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Wire {
            values: values.map(Q155::value),
            raw_values: values.map(Q155::raw),
        }
        .serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[Q155; 2], D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        let [a, b] = std::array::from_fn(|i| Q155::from_wire(wire.values[i], wire.raw_values[i]));
        Ok([
            a.map_err(serde::de::Error::custom)?,
            b.map_err(serde::de::Error::custom)?,
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Q155LaneFrame;

impl Q155LaneFrame {
    pub(super) const DISCRIMINATOR: [u8; 18] = [
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x00,
    ];
}

impl super::scalar_run::ScalarFrame for Q155LaneFrame {
    type Atom = Q155Atom;
    fn prefix_len(self) -> u64 {
        cadmpeg_core::decode::u64_from_index(Self::DISCRIMINATOR.len())
    }
}

#[cfg(test)]
mod numeric_tests {
    use super::Q155;

    fn assert_refused(value: impl Into<Option<Q155>>) {
        assert!(value.into().is_none());
    }

    #[test]
    fn q155_refuses_inexact_positive_numerator() {
        let raw = [0x20, 0, 0, 0, 0, 0, 1];
        assert_refused(Q155::from_raw(raw));
        assert!(Q155::from_wire(0.25, raw).is_err());
    }

    #[test]
    fn q155_refuses_inexact_negative_numerator() {
        let raw = [0xdf, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        assert_refused(Q155::from_raw(raw));
        assert!(Q155::from_wire(-0.25, raw).is_err());
    }
}
