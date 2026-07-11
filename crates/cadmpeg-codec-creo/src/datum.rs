// SPDX-License-Identifier: Apache-2.0
//! Standard model-space datum planes stored in `ActDatums`.

use crate::scalar;

/// An axis-aligned model-space datum plane.
///
/// The plane comes from an `ActDatums` `act_datum_geoms -> srf_array` row. Its
/// normal is a basis vector and its equation is `x_k = offset` for that axis.
#[derive(Debug, Clone, PartialEq)]
pub struct DatumPlane {
    /// The row's `geom_id`, the datum's identifier in the `ActDatums`
    /// `srf_array` namespace. `ref_planes` nested `plane_id` fields join
    /// this identifier ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#81-scalar-and-datum-tokens)).
    pub id: u32,
    /// The plane's unit normal, one of the three standard basis vectors.
    pub normal: [f64; 3],
    /// The plane's model-space offset along the axis identified by
    /// `normal`: the constant coordinate shared by both `outline` corners.
    pub offset: f64,
    /// The row's two `outline` corner points, in model-space XYZ.
    pub corners: [[f64; 3]; 2],
    /// Byte offset of the row's `outline` field in the original stream.
    pub offset_in_payload: usize,
}

/// Decode datum rows whose outline corners share one coordinate.
///
/// This promotion applies only to model-space `ActDatums` outlines.
pub fn planes(payload: &[u8]) -> Vec<DatumPlane> {
    let mut result = Vec::new();
    for offset in 0..payload.len().saturating_sub(6) {
        let id = payload[offset];
        if id == 0 || id > 0x40 || payload.get(offset + 1) != Some(&0x22) {
            continue;
        }
        if !matches!(payload.get(offset + 3), Some(0x01 | 0xf6)) {
            continue;
        }
        if !matches!(payload.get(offset + 4), Some(0 | 1 | 6 | 0xf6)) {
            continue;
        }
        let Some(values) = scalars(payload, offset + 6, 10) else {
            continue;
        };
        let outline = &values[4..];
        let differences = [
            (outline[0] - outline[3]).abs(),
            (outline[1] - outline[4]).abs(),
            (outline[2] - outline[5]).abs(),
        ];
        let (axis, difference) = differences
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .expect("invariant: `differences` is a fixed-size [f64; 3] array, so it is never empty and `min_by` always yields an element");
        if *difference > 1e-5 {
            continue;
        }
        let mut normal = [0.0; 3];
        normal[axis] = 1.0;
        result.push(DatumPlane {
            id: id as u32,
            normal,
            offset: outline[axis],
            corners: [
                [outline[0], outline[1], outline[2]],
                [outline[3], outline[4], outline[5]],
            ],
            offset_in_payload: offset,
        });
    }
    result
}

/// Decode a named zero-offset datum from matching zero outline coordinates.
pub fn named_zero_plane(payload: &[u8]) -> Option<DatumPlane> {
    let marker = b"outline\0\xf9\x02\x03";
    let outline = find(payload, marker, 0)?;
    let id_marker = b"\xe0\x01geom_id\0";
    let id_at = payload[..outline]
        .windows(id_marker.len())
        .rposition(|window| window == id_marker)?;
    let id_start = id_at + id_marker.len();
    let id = *payload.get(id_start)? as u32;
    let mut zeros = [false; 6];
    let mut cursor = outline + marker.len();
    for zero in &mut zeros {
        let head = *payload.get(cursor)?;
        match head {
            0x18 | 0x0f => {
                *zero = true;
                cursor += 1;
            }
            0x46 | 0x2d => cursor += 8,
            0x40..=0xbf | 0xd3 | 0xd7 | 0xdf => cursor += 7,
            _ => return None,
        }
    }
    let axis = (0..3).find(|axis| zeros[*axis] && zeros[*axis + 3])?;
    let mut normal = [0.0; 3];
    normal[axis] = 1.0;
    Some(DatumPlane {
        id,
        normal,
        offset: 0.0,
        corners: [[0.0; 3]; 2],
        offset_in_payload: outline,
    })
}

fn find(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

fn scalars(data: &[u8], mut offset: usize, count: usize) -> Option<Vec<f64>> {
    let mut values = Vec::with_capacity(count);
    while values.len() < count {
        let head = *data.get(offset)?;
        if head == 0x18 || head == 0x0f {
            values.push(0.0);
            offset += 1;
            continue;
        }
        let (value, next) = scalar::decode(data, offset)?;
        values.push(value);
        offset = next;
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    fn ieee8(value: f64) -> Vec<u8> {
        let mut raw = value.to_be_bytes();
        raw[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
        raw.to_vec()
    }
    #[test]
    fn decodes_constant_outline_coordinate_as_a_model_plane() {
        let mut data = vec![4, 0x22, 1, 1, 0, 0];
        data.extend([0x0f; 4]);
        data.extend(ieee8(2.0));
        data.push(0x0f);
        data.extend(ieee8(3.0));
        data.extend(ieee8(-2.0));
        data.push(0x0f);
        data.extend(ieee8(-3.0));
        assert_eq!(
            planes(&data),
            vec![DatumPlane {
                id: 4,
                normal: [0.0, 1.0, 0.0],
                offset: 0.0,
                corners: [[2.0, 0.0, 3.0], [-2.0, 0.0, -3.0]],
                offset_in_payload: 0
            }]
        );
    }

    #[test]
    fn decodes_named_standard_plane_from_zero_slots() {
        let data = b"\xe0\x01geom_id\0\x02outline\0\xf9\x02\x03\x18\x46\x08\0\0\0\0\0\0\x18\x18\x46\x08\0\0\0\0\0\0\x18";
        let plane = named_zero_plane(data).unwrap();
        assert_eq!(plane.id, 2);
        assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
        assert_eq!(plane.offset, 0.0);
    }
}
