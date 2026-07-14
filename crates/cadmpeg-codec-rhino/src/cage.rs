// SPDX-License-Identifier: Apache-2.0
//! NURBS cage payload decoding.

use std::ops::Range;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::curves::GeometryError;
use crate::wire::{scaled_coordinate, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
const MAX_DIMENSION: usize = 10_000;
const MAX_CONTROL_POINTS: usize = 1 << 20;
const MAX_SCALARS: usize = 1 << 24;
pub(crate) const CLASS: Uuid = Uuid::from_canonical([
    0x06, 0x93, 0x6a, 0xfb, 0x3d, 0x3c, 0x41, 0xac, 0xbf, 0x70, 0xc9, 0x31, 0x9f, 0xa4, 0x80, 0xa1,
]);

#[derive(Debug, Clone)]
pub(crate) struct Cage {
    pub(crate) source_range: Range<usize>,
    pub(crate) dimension: usize,
    pub(crate) rational: bool,
    pub(crate) orders: [usize; 3],
    pub(crate) counts: [usize; 3],
    pub(crate) knots: [Vec<f64>; 3],
    pub(crate) control_points: Vec<Vec<f64>>,
    pub(crate) weights: Option<Vec<f64>>,
}

fn malformed(offset: usize, message: impl Into<String>) -> GeometryError {
    GeometryError::Malformed(FramingError::Structural {
        offset,
        message: message.into(),
    })
}

fn positive(reader: &mut BoundedReader<'_>, label: &str) -> Result<usize, GeometryError> {
    let offset = reader.position();
    let value = reader.i32()?;
    if value <= 0 {
        return Err(malformed(
            offset,
            format!("NURBS cage {label} is not positive"),
        ));
    }
    usize::try_from(value).map_err(|_| malformed(offset, format!("NURBS cage {label} overflows")))
}

pub(crate) fn decode(
    data: &[u8],
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Cage, GeometryError> {
    let (cage, next) = decode_at(data, range.start, range.end, scale, archive)?;
    if next != range.end {
        return Err(malformed(range.start, "invalid NURBS cage framing"));
    }
    Ok(cage)
}

pub(crate) fn decode_at(
    data: &[u8],
    offset: usize,
    end: usize,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<(Cage, usize), GeometryError> {
    let chunk = chunk_at(data, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(malformed(offset, "invalid NURBS cage framing"));
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor != 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: chunk.body.start,
            message: format!("unsupported NURBS cage version {major}.{minor}"),
        });
    }
    let dimension = positive(&mut reader, "dimension")?;
    if dimension > MAX_DIMENSION {
        return Err(malformed(
            reader.position() - 4,
            "NURBS cage dimension exceeds cap",
        ));
    }
    let rational = match reader.i32()? {
        0 => false,
        1 => true,
        _ => {
            return Err(malformed(
                reader.position() - 4,
                "invalid NURBS cage rational flag",
            ))
        }
    };
    let orders = [
        positive(&mut reader, "U order")?,
        positive(&mut reader, "V order")?,
        positive(&mut reader, "W order")?,
    ];
    let counts = [
        positive(&mut reader, "U count")?,
        positive(&mut reader, "V count")?,
        positive(&mut reader, "W count")?,
    ];
    let orders_offset = reader.position() - 24;
    for axis in 0..3 {
        if orders[axis] < 2 || counts[axis] < orders[axis] {
            return Err(malformed(
                orders_offset + axis * 4,
                "invalid NURBS cage order and count",
            ));
        }
    }
    let control_count = counts
        .into_iter()
        .try_fold(1_usize, usize::checked_mul)
        .filter(|count| *count <= MAX_CONTROL_POINTS)
        .ok_or_else(|| malformed(reader.position(), "NURBS cage control count exceeds cap"))?;
    let mut knots: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::new());
    for axis in 0..3 {
        let knot_count = orders[axis]
            .checked_add(counts[axis])
            .and_then(|value| value.checked_sub(2))
            .ok_or_else(|| malformed(reader.position(), "NURBS cage knot count overflows"))?;
        knots[axis].reserve(knot_count);
        for _ in 0..knot_count {
            let knot = reader.f64()?;
            if !knot.is_finite() || knots[axis].last().is_some_and(|previous| knot < *previous) {
                return Err(malformed(reader.position() - 8, "invalid NURBS cage knot"));
            }
            knots[axis].push(knot);
        }
    }
    let stored_dimension = dimension + usize::from(rational);
    control_count
        .checked_mul(stored_dimension)
        .filter(|count| *count <= MAX_SCALARS && *count <= reader.remaining() / 8)
        .ok_or_else(|| malformed(reader.position(), "NURBS cage control data exceeds bound"))?;
    let mut control_points = Vec::with_capacity(control_count);
    let mut weights = rational.then(|| Vec::with_capacity(control_count));
    for _ in 0..control_count {
        let mut stored = Vec::with_capacity(stored_dimension);
        for _ in 0..stored_dimension {
            let value = reader.f64()?;
            if !value.is_finite() {
                return Err(malformed(
                    reader.position() - 8,
                    "nonfinite NURBS cage control value",
                ));
            }
            stored.push(value);
        }
        let weight = if rational {
            let weight = stored.pop().expect("rational cage has a weight");
            if weight == 0.0 {
                return Err(malformed(reader.position() - 8, "zero NURBS cage weight"));
            }
            weights
                .as_mut()
                .expect("rational weights exist")
                .push(weight);
            weight
        } else {
            1.0
        };
        let point = stored
            .into_iter()
            .map(|coordinate| {
                scaled_coordinate(coordinate / weight, scale).ok_or_else(|| {
                    malformed(reader.position(), "scaled NURBS cage coordinate is invalid")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        control_points.push(point);
    }
    if reader.remaining() != 0 {
        return Err(malformed(
            reader.position(),
            "NURBS cage has trailing bytes",
        ));
    }
    Ok((Cage {
        source_range: offset..chunk.next_offset,
        dimension,
        rational,
        orders,
        counts,
        knots,
        control_points,
        weights,
    }, chunk.next_offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive_test_support::crc_chunk;

    #[test]
    fn decodes_rational_cage_order_knots_and_u_v_w_control_order() {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend(3_i32.to_le_bytes());
        body.extend(1_i32.to_le_bytes());
        for _ in 0..3 {
            body.extend(2_i32.to_le_bytes());
        }
        for _ in 0..3 {
            body.extend(2_i32.to_le_bytes());
        }
        for axis in 0..3 {
            body.extend(0.0_f64.to_le_bytes());
            body.extend((axis as f64 + 1.0).to_le_bytes());
        }
        for index in 0..8 {
            let weight = if index == 7 { 2.0 } else { 1.0 };
            for coordinate in [index as f64 * weight, 0.0, 0.0, weight] {
                body.extend(coordinate.to_le_bytes());
            }
        }
        let bytes = crc_chunk(ANONYMOUS, &body);
        let cage = decode(&bytes, 0..bytes.len(), 10.0, ArchiveVersion::V8).unwrap();
        assert_eq!(cage.orders, [2, 2, 2]);
        assert_eq!(cage.counts, [2, 2, 2]);
        assert_eq!(cage.knots[2], [0.0, 3.0]);
        assert_eq!(cage.control_points[7], [70.0, 0.0, 0.0]);
        assert_eq!(cage.weights.as_ref().unwrap()[7], 2.0);
    }
}
