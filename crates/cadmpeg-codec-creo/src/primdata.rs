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

/// One complete position-only triangle-strip primitive.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimitiveTriangleStrip {
    /// Byte offset of `value(prim_tristripsetwithatt)` in the expanded section.
    pub offset: usize,
    /// Consecutive model-space positions.
    pub positions: Vec<[f64; 3]>,
    /// Per-vertex normals when the primitive uses the interleaved normal and
    /// position lane.
    pub normals: Vec<[f64; 3]>,
    /// Vertex count of each consecutive triangle strip.
    pub strip_lengths: Vec<u32>,
}

/// Decode named position-only triangle-strip primitives.
pub fn triangle_strips(data: &[u8]) -> Vec<PrimitiveTriangleStrip> {
    const RECORD: &[u8] = b"value(prim_tristripsetwithatt)\0";
    const ACCUM: &[u8] = b"\xe0\x01p_accum_set_size\0";
    let mut strips = Vec::new();
    for (offset, _) in data
        .windows(RECORD.len())
        .enumerate()
        .filter(|(_, window)| *window == RECORD)
    {
        let end = data[offset + RECORD.len()..]
            .windows(b"\xe0\x00value(".len())
            .position(|window| window == b"\xe0\x00value(")
            .map_or(data.len(), |relative| offset + RECORD.len() + relative);
        let record = &data[offset..end];
        let Some(accum) = record
            .windows(ACCUM.len())
            .position(|window| window == ACCUM)
            .map(|relative| relative + ACCUM.len())
        else {
            continue;
        };
        if record.get(accum) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (count, mut cursor) = psb::compact_int(record, accum + 1);
        let mut cumulative = Vec::new();
        for _ in 0..count {
            let (value, next) = psb::compact_int(record, cursor);
            if next == cursor {
                cumulative.clear();
                break;
            }
            cumulative.push(value);
            cursor = next;
        }
        let arrays = scalar_arrays(record);
        let Some((positions, normals)) = arrays.iter().find_map(|array| {
            if array.field == "mv_p_xyz" && array.values.len() % 3 == 0 {
                let positions: Vec<[f64; 3]> = array
                    .values
                    .chunks_exact(3)
                    .map(|point| [point[0], point[1], point[2]])
                    .collect();
                return Some((positions, Vec::new()));
            }
            if array.field == "mv_p_NxNyNzxyz" && array.values.len() % 6 == 0 {
                let normals: Vec<[f64; 3]> = array
                    .values
                    .chunks_exact(6)
                    .map(|tuple| [tuple[0], tuple[1], tuple[2]])
                    .collect();
                let positions: Vec<[f64; 3]> = array
                    .values
                    .chunks_exact(6)
                    .map(|tuple| [tuple[3], tuple[4], tuple[5]])
                    .collect();
                return Some((positions, normals));
            }
            None
        }) else {
            continue;
        };
        let vertex_count = u32::try_from(positions.len()).ok();
        if cumulative.last().copied() != vertex_count {
            continue;
        }
        let mut previous = 0;
        let mut strip_lengths = Vec::with_capacity(cumulative.len());
        for current in cumulative {
            let Some(length) = current.checked_sub(previous).filter(|length| *length >= 3) else {
                strip_lengths.clear();
                break;
            };
            strip_lengths.push(length);
            previous = current;
        }
        if strip_lengths.is_empty() {
            continue;
        }
        strips.push(PrimitiveTriangleStrip {
            offset,
            positions,
            normals,
            strip_lengths,
        });
    }
    strips
}

/// Decode model-space scalar arrays from an expanded primitive-data section.
///
/// Primitive coordinates use a float32 lane distinct from the float64 lanes
/// in analytic geometry records. `00` is zero, `00 28 00` is a three-slot
/// positive-Y unit vector, and signed four-byte values replace the IEEE-754
/// high byte with a compact exponent byte. Only complete arrays whose declared
/// count is satisfied are returned.
pub fn scalar_arrays(data: &[u8]) -> Vec<PrimitiveScalarArray> {
    const FIELDS: &[&str] = &["p1", "p2", "pts", "mv_p_xyz", "mv_p_NxNyNzxyz"];
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
            let (count, start) = psb::compact_int(data, opener + 1);
            if start == opener + 1 {
                continue;
            }
            let Ok(capacity) = usize::try_from(count) else {
                continue;
            };
            let mut values = Vec::with_capacity(capacity);
            let mut cursor = psb::Cursor::at(data, start);
            while values.len() < capacity {
                if capacity - values.len() >= 3 && cursor.take_slice_if(&[0x00, 0x28, 0x00]) {
                    values.extend([0.0, 1.0, 0.0]);
                    continue;
                }
                let Some(value) = cursor.take_with(primitive_scalar) else {
                    break;
                };
                values.push(value);
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

    #[test]
    fn decodes_position_only_array() {
        let bytes = named(
            "mv_p_xyz",
            &[
                0x48, 0x21, 0x96, 0xec, 0x3a, 0xa2, 0xe2, 0xc4, 0x48, 0x2a, 0xbb, 0x34,
            ],
            3,
        );
        let arrays = scalar_arrays(&bytes);
        assert_eq!(arrays.len(), 1);
        assert_eq!(arrays[0].field, "mv_p_xyz");
        assert_eq!(arrays[0].values.len(), 3);
    }

    #[test]
    fn decodes_named_triangle_strip() {
        let mut bytes =
            b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8\x01\x03".to_vec();
        bytes.extend(named(
            "mv_p_xyz",
            &[
                0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
            9,
        ));
        let strips = triangle_strips(&bytes);
        assert_eq!(strips.len(), 1);
        assert_eq!(strips[0].positions.len(), 3);
        assert!(strips[0].normals.is_empty());
        assert_eq!(strips[0].strip_lengths, [3]);
    }

    #[test]
    fn decodes_interleaved_triangle_strip_positions_and_normals() {
        let mut bytes =
            b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8\x01\x03".to_vec();
        let tuple = [
            0x00, 0x28, 0x00, 0x00, 0x00, 0x00, // normal, position 0
            0x00, 0x28, 0x00, 0x46, 0x80, 0x00, 0x00, 0x00, 0x00, // normal, position x
            0x00, 0x28, 0x00, 0x00, 0x46, 0x80, 0x00, 0x00, 0x00, // normal, position y
        ];
        bytes.extend(named("mv_p_NxNyNzxyz", &tuple, 18));

        let strips = triangle_strips(&bytes);
        assert_eq!(strips.len(), 1);
        assert_eq!(
            strips[0].positions,
            [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        assert_eq!(strips[0].normals, [[0.0, 1.0, 0.0]; 3]);
        assert_eq!(strips[0].strip_lengths, [3]);
    }
}
