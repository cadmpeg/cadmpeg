// SPDX-License-Identifier: Apache-2.0
//! Signed Q1.55 scalars used by datum-CSYS pairs.

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
        let high = i32::try_from(signed.div_euclid(1_i64 << 24)).ok()?;
        let low = u32::try_from(signed.rem_euclid(1_i64 << 24)).ok()?;
        let value = f64::from(high) / 2_147_483_648.0 + f64::from(low) / 36_028_797_018_963_968.0;
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
            Self::from_raw(raw).ok_or("signed Q1.55 raw_values must form a valid signed atom")?;
        if scalar.value().to_bits() != value.to_bits() {
            return Err("values must match signed Q1.55 raw_values");
        }
        Ok(scalar)
    }
}

pub(crate) mod pair_wire {
    use super::Q155;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        values: [f64; 2],
        raw_values: [cadmpeg_ir::native::bytes::NativeBytes<[u8; 7]>; 2],
    }

    pub(crate) fn serialize<S: Serializer>(
        values: &[Q155; 2],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Wire {
            values: values.map(Q155::value),
            raw_values: (values.map(Q155::raw)).map(Into::into),
        }
        .serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[Q155; 2], D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        let [a, b] = std::array::from_fn(|i| Q155::from_wire(wire.values[i], *wire.raw_values[i]));
        Ok([
            a.map_err(serde::de::Error::custom)?,
            b.map_err(serde::de::Error::custom)?,
        ])
    }
}

#[cfg(test)]
mod numeric_tests {
    use super::Q155;

    fn assert_projection(raw: [u8; 7], expected_bits: u64) {
        let scalar = Q155::from_raw(raw).expect("every signed seven-byte atom is valid");
        assert_eq!(scalar.raw(), raw);
        assert_eq!(scalar.value().to_bits(), expected_bits);
        assert_eq!(Q155::from_wire(scalar.value(), raw), Ok(scalar));
        assert!(Q155::from_wire(f64::from_bits(expected_bits ^ 1), raw).is_err());
    }

    #[test]
    fn q155_rounds_inexact_numerators_and_preserves_raw_atoms() {
        for (raw, expected_bits) in [
            ([0x20, 0, 0, 0, 0, 0, 1], 0x3fd0_0000_0000_0000),
            (
                [0xdf, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
                0xbfd0_0000_0000_0000,
            ),
            (
                [0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
                0x3ff0_0000_0000_0000,
            ),
            ([0x80, 0, 0, 0, 0, 0, 0], 0xbff0_0000_0000_0000),
            ([0, 0, 0, 0, 0, 0, 0], 0),
            (
                [0x74, 0x46, 0xa1, 0xf1, 0x9b, 0x53, 0x38],
                0x3fed_11a8_7c66_d4ce,
            ),
            (
                [0x50, 0xff, 0x89, 0xab, 0xac, 0x0d, 0xee],
                0x3fe4_3fe2_6aeb_037c,
            ),
            (
                [0x6b, 0xda, 0x10, 0xb0, 0x03, 0x73, 0x40],
                0x3fea_f684_2c00_dcd0,
            ),
            (
                [0x42, 0x1f, 0x13, 0x57, 0x58, 0x1b, 0x36],
                0x3fe0_87c4_d5d6_06ce,
            ),
        ] {
            assert_projection(raw, expected_bits);
        }
    }

    #[test]
    fn q155_pair_wire_roundtrips_rounded_values_and_exact_source_bytes() {
        #[derive(Debug, serde::Serialize, serde::Deserialize)]
        struct Pair {
            #[serde(with = "super::pair_wire")]
            scalars: [Q155; 2],
        }

        let raw = [
            [0x74, 0x46, 0xa1, 0xf1, 0x9b, 0x53, 0x38],
            [0x50, 0xff, 0x89, 0xab, 0xac, 0x0d, 0xee],
        ];
        let scalars = raw.map(|bytes| Q155::from_raw(bytes).expect("valid signed atom"));
        let encoded = serde_json::to_value(Pair { scalars }).expect("serialize pair");
        assert_eq!(encoded["scalars"]["raw_values"][0], "7446a1f19b5338");
        assert_eq!(encoded["scalars"]["raw_values"][1], "50ff89abac0dee");
        let decoded: Pair = serde_json::from_value(encoded.clone()).expect("deserialize pair");
        assert_eq!(decoded.scalars, scalars);

        let mut wrong_value = encoded.clone();
        wrong_value["scalars"]["values"][1] = serde_json::json!(0.5);
        assert!(serde_json::from_value::<Pair>(wrong_value).is_err());
        let mut wrong_raw = encoded;
        wrong_raw["scalars"]["raw_values"][1] = serde_json::json!("00000000000000");
        assert!(serde_json::from_value::<Pair>(wrong_raw).is_err());
    }
}
