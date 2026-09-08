// SPDX-License-Identifier: Apache-2.0
//! Signed Q1.55 scalars and their exact atom markers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Q155([u8; 7]);

impl Q155 {
    pub(crate) fn from_raw(raw: [u8; 7]) -> Self {
        Self(raw)
    }

    pub(crate) fn raw(self) -> [u8; 7] {
        self.0
    }

    pub(crate) fn value(self) -> f64 {
        let unsigned = self
            .0
            .into_iter()
            .fold(0_u64, |value, byte| (value << 8) | u64::from(byte));
        let signed = if unsigned & (1_u64 << 55) == 0 {
            unsigned as i64
        } else {
            (unsigned as i64) - (1_i64 << 56)
        };
        signed as f64 / (1_u64 << 55) as f64
    }

    pub(crate) fn from_wire(value: f64, raw: [u8; 7]) -> Result<Self, &'static str> {
        let scalar = Self(raw);
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
    pub(crate) const DISCRIMINATOR: [u8; 18] = [
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x00,
    ];
}

impl super::scalar_run::ScalarFrame for Q155LaneFrame {
    type Atom = Q155Atom;
    fn prefix_len(self) -> u64 {
        Self::DISCRIMINATOR.len() as u64
    }
}
