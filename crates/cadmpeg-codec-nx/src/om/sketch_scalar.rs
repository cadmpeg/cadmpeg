// SPDX-License-Identifier: Apache-2.0
//! Sketch scalars with the implicit `30` marker and one-quarter scale.

use super::scalar::ShiftedBinary32;
use serde::{Deserialize, Serialize};

const SKETCH_FIXED_ATOM_SCALE: f64 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SketchScaledAtom([u8; 7]);

impl SketchScaledAtom {
    pub(crate) fn from_raw(raw: [u8; 7]) -> Self {
        Self(raw)
    }

    pub(crate) fn raw(self) -> [u8; 7] {
        self.0
    }

    pub(crate) fn value(self) -> f64 {
        let mut encoded = [0_u8; 8];
        encoded[0] = 0x40;
        encoded[1..].copy_from_slice(&self.0);
        // endian-exception: reconstructed-scalar
        f64::from_be_bytes(encoded) * SKETCH_FIXED_ATOM_SCALE
    }

    pub(crate) fn from_wire(value: f64, raw: [u8; 7]) -> Result<Self, &'static str> {
        let scalar = Self(raw);
        if scalar.value().to_bits() != value.to_bits() {
            return Err("values must match scaled shifted-binary64 raw_values");
        }
        Ok(scalar)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MixedWire", into = "MixedWire")]
pub(crate) struct SketchMixedScalars {
    pub(crate) fixed: SketchScaledAtom,
    pub(crate) binary32: ShiftedBinary32,
}

#[derive(Serialize, Deserialize)]
// Field names are the native record serialized keys.
#[allow(clippy::struct_field_names)]
struct MixedWire {
    fixed_value: f64,
    binary32_value: f64,
    fixed_raw_value: [u8; 7],
    binary32_raw_value: [u8; 4],
}

impl From<SketchMixedScalars> for MixedWire {
    fn from(scalars: SketchMixedScalars) -> Self {
        Self {
            fixed_value: scalars.fixed.value(),
            binary32_value: scalars.binary32.value(),
            fixed_raw_value: scalars.fixed.raw(),
            binary32_raw_value: scalars.binary32.raw(),
        }
    }
}

impl TryFrom<MixedWire> for SketchMixedScalars {
    type Error = String;

    fn try_from(wire: MixedWire) -> Result<Self, Self::Error> {
        Ok(Self {
            fixed: SketchScaledAtom::from_wire(wire.fixed_value, wire.fixed_raw_value)
                .map_err(|error| format!("fixed_value/fixed_raw_value: {error}"))?,
            binary32: ShiftedBinary32::from_wire(wire.binary32_value, &wire.binary32_raw_value)
                .map_err(|error| format!("binary32_value/binary32_raw_value: {error}"))?,
        })
    }
}

pub(crate) mod pair_wire {
    use super::SketchScaledAtom;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        values: [f64; 2],
        raw_values: [[u8; 7]; 2],
    }

    pub(crate) fn serialize<S: Serializer>(
        values: &[SketchScaledAtom; 2],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Wire {
            values: values.map(SketchScaledAtom::value),
            raw_values: values.map(SketchScaledAtom::raw),
        }
        .serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[SketchScaledAtom; 2], D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        let [a, b] = std::array::from_fn(|i| {
            SketchScaledAtom::from_wire(wire.values[i], wire.raw_values[i])
        });
        Ok([
            a.map_err(serde::de::Error::custom)?,
            b.map_err(serde::de::Error::custom)?,
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchScalarLaneForm {
    Form03,
    Form07,
}

impl SketchScalarLaneForm {
    pub(crate) fn discriminator(self) -> &'static [u8] {
        match self {
            Self::Form03 => &[
                0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86,
                0x81, 0x02, 0x00, 0x01, 0x00,
            ],
            Self::Form07 => &[
                0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86,
                0x02, 0x00, 0x01, 0x00,
            ],
        }
    }

    pub(crate) fn from_discriminator(bytes: &[u8]) -> Result<Self, &'static str> {
        [Self::Form03, Self::Form07]
            .into_iter()
            .find(|form| form.discriminator() == bytes)
            .ok_or("discriminator must select a sketch scalar lane form")
    }
}

impl super::scalar_run::ScalarFrame for SketchScalarLaneForm {
    type Atom = super::scalar::ShiftedScalar;
    fn prefix_len(self) -> u64 {
        self.discriminator().len() as u64
    }
}
