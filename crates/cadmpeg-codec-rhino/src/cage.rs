// SPDX-License-Identifier: Apache-2.0
//! NURBS cage payload decoding.
#![deny(clippy::disallowed_methods)]

use std::ops::Range;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::View;

use crate::chunks::{chunk_at, ArchiveVersion, FramingError};
use crate::curves::GeometryError;
use crate::mesh::MeshExpand;
use crate::wire::{scaled_coordinate, ExactVec, Uuid};

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

fn refused(offset: usize, error: &CodecError) -> GeometryError {
    malformed(offset, format!("NURBS cage allocation refused: {error}"))
}

fn req_i32(view: &mut View<'_>) -> Result<i32, GeometryError> {
    let offset = view.position();
    view.req_i32_le()
        .map_err(|_| malformed(offset, "NURBS cage record truncated"))
}

fn req_f64(view: &mut View<'_>) -> Result<f64, GeometryError> {
    let offset = view.position();
    view.req_f64_le()
        .map_err(|_| malformed(offset, "NURBS cage record truncated"))
}

fn positive(view: &mut View<'_>, label: &str) -> Result<usize, GeometryError> {
    let offset = view.position();
    let value = req_i32(view)?;
    if value <= 0 {
        return Err(malformed(
            offset,
            format!("NURBS cage {label} is not positive"),
        ));
    }
    usize::try_from(value).map_err(|_| malformed(offset, format!("NURBS cage {label} overflows")))
}

pub(crate) fn decode(
    expand: MeshExpand<'_>,
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Cage, GeometryError> {
    let (cage, next) = decode_at(expand, range.start, range.end, scale, archive)?;
    if next != range.end {
        return Err(malformed(range.start, "invalid NURBS cage framing"));
    }
    Ok(cage)
}

pub(crate) fn decode_at(
    expand: MeshExpand<'_>,
    offset: usize,
    end: usize,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<(Cage, usize), GeometryError> {
    let data = expand.data();
    let chunk = chunk_at(data, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(malformed(offset, "invalid NURBS cage framing"));
    }

    let mut body = expand
        .root()
        .child(chunk.body.start, chunk.body.end)
        .ok_or_else(|| malformed(chunk.body.start, "NURBS cage body out of range"))?;

    let major = req_i32(&mut body)?;
    let minor = req_i32(&mut body)?;
    if major != 1 || minor != 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: chunk.body.start,
            message: format!("unsupported NURBS cage version {major}.{minor}"),
        });
    }
    let dimension = positive(&mut body, "dimension")?;
    if dimension > MAX_DIMENSION {
        return Err(malformed(
            body.position() - 4,
            "NURBS cage dimension exceeds cap",
        ));
    }
    let rational = match req_i32(&mut body)? {
        0 => false,
        1 => true,
        _ => {
            return Err(malformed(
                body.position() - 4,
                "invalid NURBS cage rational flag",
            ))
        }
    };
    let orders = [
        positive(&mut body, "U order")?,
        positive(&mut body, "V order")?,
        positive(&mut body, "W order")?,
    ];
    let counts = [
        positive(&mut body, "U count")?,
        positive(&mut body, "V count")?,
        positive(&mut body, "W count")?,
    ];
    let orders_offset = body.position() - 24;
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
        .ok_or_else(|| malformed(body.position(), "NURBS cage control count exceeds cap"))?;

    let mut knots: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::new());
    for axis in 0..3 {
        let knot_count = orders[axis]
            .checked_add(counts[axis])
            .and_then(|value| value.checked_sub(2))
            .ok_or_else(|| malformed(body.position(), "NURBS cage knot count overflows"))?;
        let bound = body
            .counted(knot_count as u64, 8)
            .ok_or_else(|| malformed(body.position(), "NURBS cage knot vector truncated"))?;
        let mut reserved =
            ExactVec::<f64>::new(bound).map_err(|error| refused(body.position(), &error))?;
        let mut previous: Option<f64> = None;
        for _ in 0..knot_count {
            let knot = req_f64(&mut body)?;
            if !knot.is_finite() || previous.is_some_and(|last| knot < last) {
                return Err(malformed(body.position() - 8, "invalid NURBS cage knot"));
            }
            previous = Some(knot);
            reserved
                .push(knot)
                .map_err(|error| refused(body.position(), &error))?;
        }
        knots[axis] = reserved
            .finish()
            .map_err(|error| refused(body.position(), &error))?;
    }

    let stored_dimension = dimension + usize::from(rational);
    let _total_scalars = control_count
        .checked_mul(stored_dimension)
        .filter(|count| *count <= MAX_SCALARS && *count <= body.remaining() / 8)
        .ok_or_else(|| malformed(body.position(), "NURBS cage control data exceeds bound"))?;

    let control_bound = body
        .counted(control_count as u64, stored_dimension * 8)
        .ok_or_else(|| malformed(body.position(), "NURBS cage control net truncated"))?;
    let mut control_points = ExactVec::<Vec<f64>>::new(control_bound)
        .map_err(|error| refused(body.position(), &error))?;
    let mut weights = if rational {
        let mut weights = Vec::new();
        weights
            .try_reserve_exact(control_count)
            .map_err(|_| malformed(body.position(), "NURBS cage weight allocation failed"))?;
        Some(weights)
    } else {
        None
    };
    for _ in 0..control_count {
        let tuple_bound = body
            .counted(stored_dimension as u64, 8)
            .ok_or_else(|| malformed(body.position(), "NURBS cage coordinate tuple truncated"))?;
        let mut stored =
            ExactVec::<f64>::new(tuple_bound).map_err(|error| refused(body.position(), &error))?;
        for _ in 0..stored_dimension {
            let value = req_f64(&mut body)?;
            if !value.is_finite() {
                return Err(malformed(
                    body.position() - 8,
                    "nonfinite NURBS cage control value",
                ));
            }
            stored
                .push(value)
                .map_err(|error| refused(body.position(), &error))?;
        }
        let mut stored = stored
            .finish()
            .map_err(|error| refused(body.position(), &error))?;
        let weight = if rational {
            let weight = stored.pop().expect("rational cage has a weight");
            if weight == 0.0 {
                return Err(malformed(body.position() - 8, "zero NURBS cage weight"));
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
                    malformed(body.position(), "scaled NURBS cage coordinate is invalid")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        control_points
            .push(point)
            .map_err(|error| refused(body.position(), &error))?;
    }
    if body.remaining() != 0 {
        return Err(malformed(body.position(), "NURBS cage has trailing bytes"));
    }
    let control_points = control_points
        .finish()
        .map_err(|error| refused(body.position(), &error))?;
    Ok((
        Cage {
            source_range: offset..chunk.next_offset,
            dimension,
            rational,
            orders,
            counts,
            knots,
            control_points,
            weights,
        },
        chunk.next_offset,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive_test_support::crc_chunk;

    fn rational_cage_body() -> Vec<u8> {
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
        body
    }

    #[test]
    fn decodes_rational_cage_order_knots_and_u_v_w_control_order() {
        let bytes = crc_chunk(ANONYMOUS, &rational_cage_body());
        let cage = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(expand, 0..bytes.len(), 10.0, ArchiveVersion::V8)
        })
        .unwrap();
        assert_eq!(cage.orders, [2, 2, 2]);
        assert_eq!(cage.counts, [2, 2, 2]);
        assert_eq!(cage.knots[2], [0.0, 3.0]);
        assert_eq!(cage.control_points[7], [70.0, 0.0, 0.0]);
        assert_eq!(cage.weights.as_ref().unwrap()[7], 2.0);
    }

    #[test]
    fn truncating_the_control_net_is_rejected_at_the_record_boundary() {
        // Drop the final control-point tuple so the count-framed control loop
        // runs past the record body's proven window.
        let mut body = rational_cage_body();
        body.truncate(body.len() - 32);
        let bytes = crc_chunk(ANONYMOUS, &body);
        assert!(crate::decode::with_expand_bytes(&bytes, |expand| decode(
            expand,
            0..bytes.len(),
            10.0,
            ArchiveVersion::V8
        ))
        .is_err());
    }
}
