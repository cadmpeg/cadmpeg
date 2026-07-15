// SPDX-License-Identifier: Apache-2.0
//! Typed geometry arrays from expanded `SolidPrimdata` sections.

use crate::psb;

/// One bounded, named scalar array in a primitive record.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimitiveScalarArray {
    /// Named field containing the array.
    pub field: String,
    /// Byte offset of the named-record header in the expanded section.
    pub offset: usize,
    /// Declared scalar count.
    pub count: u32,
    /// Completely decoded scalar values.
    pub values: Vec<f64>,
}

/// Decode model-space scalar arrays from an expanded primitive-data section.
///
/// Primitive coordinates use a float32 lane distinct from the float64 lanes
/// in analytic geometry records. `00` is zero, `28 00` is one, and signed
/// four-byte values replace the IEEE-754 high byte with a compact exponent
/// byte. Only complete arrays whose declared count is satisfied are returned.
pub fn scalar_arrays(data: &[u8]) -> Vec<PrimitiveScalarArray> {
    const FIELDS: &[&str] = &["p1", "p2", "pts", "mv_p_NxNyNzxyz"];
    let mut arrays = Vec::new();
    for field in FIELDS {
        let mut marker = vec![psb::token::NAMED_RECORD, 0x06];
        marker.extend_from_slice(field.as_bytes());
        marker.push(0);
        for (offset, _) in data
            .windows(marker.len())
            .enumerate()
            .filter(|(_, window)| *window == marker)
        {
            let opener = offset + marker.len();
            if data.get(opener) != Some(&psb::token::ARRAY_OPEN) {
                continue;
            }
            let (count, mut cursor) = psb::compact_int(data, opener + 1);
            if cursor == opener + 1 {
                continue;
            }
            let Ok(capacity) = usize::try_from(count) else {
                continue;
            };
            let mut values = Vec::with_capacity(capacity);
            while values.len() < capacity {
                if capacity - values.len() >= 3
                    && data.get(cursor..cursor + 3) == Some(&[0x00, 0x28, 0x00])
                {
                    values.extend([0.0, 1.0, 0.0]);
                    cursor += 3;
                    continue;
                }
                let Some((value, next)) = primitive_scalar(data, cursor) else {
                    break;
                };
                values.push(value);
                cursor = next;
            }
            if values.len() == capacity && values.iter().all(|value| value.is_finite()) {
                arrays.push(PrimitiveScalarArray {
                    field: (*field).to_string(),
                    offset,
                    count,
                    values,
                });
            }
        }
    }
    arrays.sort_by_key(|array| array.offset);
    arrays
}

fn primitive_scalar(data: &[u8], offset: usize) -> Option<(f64, usize)> {
    match data.get(offset..)? {
        [0x00, ..] => Some((0.0, offset + 1)),
        [head @ (0x36..=0x3d | 0x46..=0x4d), b1, b2, b3, ..] => {
            let ieee_high = if *head >= 0x46 {
                head.checked_sub(7)?
            } else {
                head.checked_add(0x89)?
            };
            let value = f32::from_be_bytes([ieee_high, *b1, *b2, *b3]) as f64;
            Some((value, offset + 4))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str, values: &[u8], count: u8) -> Vec<u8> {
        let mut bytes = vec![0xe0, 0x06];
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&[0, 0xf8, count]);
        bytes.extend_from_slice(values);
        bytes
    }

    #[test]
    fn decodes_complete_primitive_float_array() {
        let bytes = named(
            "p1",
            &[0x00, 0x48, 0xa6, 0x66, 0x66, 0x38, 0x86, 0x66, 0x66],
            3,
        );
        let arrays = scalar_arrays(&bytes);
        assert_eq!(arrays.len(), 1);
        assert_eq!(arrays[0].values[0], 0.0);
        assert!((arrays[0].values[1] - 20.8).abs() < 1.0e-5);
        assert!((arrays[0].values[2] + 16.8).abs() < 1.0e-5);
    }

    #[test]
    fn rejects_truncated_declared_array() {
        let bytes = named("pts", &[0x48, 0xa6, 0x66, 0x66], 2);
        assert!(scalar_arrays(&bytes).is_empty());
    }

    #[test]
    fn decodes_interleaved_normal_position_array() {
        let tuple = [
            0x00, 0x28, 0x00, 0x38, 0xa6, 0x66, 0x66, 0x48, 0x93, 0x33, 0x33, 0x38, 0x86, 0x66,
            0x66,
        ];
        let bytes = named("mv_p_NxNyNzxyz", &tuple, 6);
        let arrays = scalar_arrays(&bytes);
        assert_eq!(arrays.len(), 1);
        assert_eq!(arrays[0].count, 6);
    }
}
