// SPDX-License-Identifier: Apache-2.0
//! NURBS cage payload decoding.

use std::ops::Range;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal};

use crate::chunks::{chunk_at, ArchiveVersion};
use crate::curves::GeometryError;
use crate::mesh::MeshExpand;
use crate::settings::MillimeterScale;
use crate::wire::Uuid;
use cadmpeg_core::decode::collect::ExactVec;

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
    pub(crate) orders: [usize; 3],
    pub(crate) counts: [usize; 3],
    pub(crate) knots: [Vec<FiniteReal>; 3],
    pub(crate) control_points: Vec<Vec<FiniteReal>>,
    pub(crate) weights: Option<Vec<NonZeroReal>>,
}

impl Cage {
    pub(crate) const fn rational(&self) -> bool {
        self.weights.is_some()
    }
}

fn refused(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    offset: usize,
    error: &CodecError,
) -> Result<GeometryError, cadmpeg_core::CodecError> {
    Ok(match error {
        CodecError::ResourceLimit(limit) => GeometryError::Codec(CodecError::ResourceLimit(*limit)),
        _ => GeometryError::malformed(
            offset,
            ctx.format_retained(
                format_args!("NURBS cage allocation refused: {error}"),
                "Rhino refused text",
            )?,
        ),
    })
}

fn req_i32(view: &mut View<'_>) -> Result<i32, GeometryError> {
    let offset = view.position();
    view.req_i32_le()
        .map_err(|_| GeometryError::malformed(offset, "NURBS cage record truncated"))
}

fn req_f64(view: &mut View<'_>) -> Result<f64, GeometryError> {
    let offset = view.position();
    view.req_f64_le()
        .map_err(|_| GeometryError::malformed(offset, "NURBS cage record truncated"))
}

fn positive(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    view: &mut View<'_>,
    label: &str,
) -> Result<usize, GeometryError> {
    let offset = view.position();
    let value = req_i32(view)?;
    if value <= 0 {
        return Err(GeometryError::malformed(
            offset,
            ctx.format_retained(
                format_args!("NURBS cage {label} is not positive"),
                "Rhino positive text",
            )?,
        ));
    }
    usize::try_from(value).or_else(|_| {
        Err(GeometryError::malformed(
            offset,
            ctx.format_retained(
                format_args!("NURBS cage {label} overflows"),
                "Rhino positive text",
            )?,
        ))
    })
}

pub(crate) fn decode(
    expand: MeshExpand<'_>,
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<Cage, GeometryError> {
    let (cage, next) = decode_at(expand, range.start, range.end, scale, archive)?;
    if next != range.end {
        return Err(GeometryError::malformed(
            range.start,
            "invalid NURBS cage framing",
        ));
    }
    Ok(cage)
}

pub(crate) fn decode_at(
    expand: MeshExpand<'_>,
    offset: usize,
    end: usize,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<(Cage, usize), GeometryError> {
    let data = expand.data();
    let chunk = chunk_at(data, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(GeometryError::malformed(
            offset,
            "invalid NURBS cage framing",
        ));
    }

    let mut body = expand
        .root()
        .child(chunk.body().start, chunk.body().end)
        .ok_or_else(|| {
            GeometryError::malformed(chunk.body().start, "NURBS cage body out of range")
        })?;

    let major = req_i32(&mut body)?;
    let minor = req_i32(&mut body)?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: chunk.body().start,
            message: format!("unsupported NURBS cage version {major}.{minor}"),
        });
    }
    let dimension = positive(expand.ctx(), &mut body, "dimension")?;
    if dimension > MAX_DIMENSION {
        return Err(GeometryError::malformed(
            body.position() - 4,
            "NURBS cage dimension exceeds cap",
        ));
    }
    let rational = match req_i32(&mut body)? {
        0 => false,
        1 => true,
        _ => {
            return Err(GeometryError::malformed(
                body.position() - 4,
                "invalid NURBS cage rational flag",
            ))
        }
    };
    let orders = [
        positive(expand.ctx(), &mut body, "U order")?,
        positive(expand.ctx(), &mut body, "V order")?,
        positive(expand.ctx(), &mut body, "W order")?,
    ];
    let counts = [
        positive(expand.ctx(), &mut body, "U count")?,
        positive(expand.ctx(), &mut body, "V count")?,
        positive(expand.ctx(), &mut body, "W count")?,
    ];
    let orders_offset = body.position() - 24;
    for axis in 0..3 {
        if orders[axis] < 2 || counts[axis] < orders[axis] {
            return Err(GeometryError::malformed(
                orders_offset + axis * 4,
                "invalid NURBS cage order and count",
            ));
        }
    }
    let control_count = counts
        .into_iter()
        .try_fold(1_usize, usize::checked_mul)
        .filter(|count| *count <= MAX_CONTROL_POINTS)
        .ok_or_else(|| {
            GeometryError::malformed(body.position(), "NURBS cage control count exceeds cap")
        })?;

    let mut knots: [Vec<FiniteReal>; 3] = std::array::from_fn(|_| Vec::new());
    for axis in 0..3 {
        let knot_count = orders[axis]
            .checked_add(counts[axis])
            .and_then(|value| value.checked_sub(2))
            .ok_or_else(|| {
                GeometryError::malformed(body.position(), "NURBS cage knot count overflows")
            })?;
        let bound = body
            .counted(cadmpeg_core::decode::u64_from_index(knot_count), 8)
            .ok_or_else(|| {
                GeometryError::malformed(body.position(), "NURBS cage knot vector truncated")
            })?;
        let mut reserved =
            ExactVec::<FiniteReal>::new(expand.ctx(), bound, "Rhino cage knot values")
                .or_else(|error| Err(refused(expand.ctx(), body.position(), &error)?))?;
        let mut previous: Option<FiniteReal> = None;
        for _ in 0..knot_count {
            let knot = req_f64(&mut body)?;
            let Some(knot) = FiniteReal::new(knot) else {
                return Err(GeometryError::malformed(
                    body.position() - 8,
                    "invalid NURBS cage knot",
                ));
            };
            if previous.is_some_and(|last| knot.get() < last.get()) {
                return Err(GeometryError::malformed(
                    body.position() - 8,
                    "invalid NURBS cage knot",
                ));
            }
            previous = Some(knot);
            reserved
                .push(expand.ctx(), knot, "Rhino cage knot values")
                .or_else(|error| Err(refused(expand.ctx(), body.position(), &error)?))?;
        }
        knots[axis] = reserved
            .finish()
            .or_else(|error| Err(refused(expand.ctx(), body.position(), &error)?))?;
    }

    let stored_dimension = dimension + usize::from(rational);
    let _total_scalars = control_count
        .checked_mul(stored_dimension)
        .filter(|count| *count <= MAX_SCALARS && *count <= body.remaining() / 8)
        .ok_or_else(|| {
            GeometryError::malformed(body.position(), "NURBS cage control data exceeds bound")
        })?;

    let control_bound = body
        .counted(
            cadmpeg_core::decode::u64_from_index(control_count),
            stored_dimension * 8,
        )
        .ok_or_else(|| {
            GeometryError::malformed(body.position(), "NURBS cage control net truncated")
        })?;
    let mut control_points =
        ExactVec::<Vec<FiniteReal>>::new(expand.ctx(), control_bound, "Rhino cage control points")
            .or_else(|error| Err(refused(expand.ctx(), body.position(), &error)?))?;
    let mut weights = if rational {
        Some(
            expand
                .ctx()
                .collection_vec(control_count, "Rhino cage weights")
                .map_err(crate::curves::GeometryError::from)?,
        )
    } else {
        None
    };
    for _ in 0..control_count {
        let tuple_bound = body
            .counted(cadmpeg_core::decode::u64_from_index(dimension), 8)
            .ok_or_else(|| {
                GeometryError::malformed(body.position(), "NURBS cage coordinate tuple truncated")
            })?;
        let mut stored =
            ExactVec::<FiniteReal>::new(expand.ctx(), tuple_bound, "Rhino cage coordinate tuple")
                .or_else(|error| Err(refused(expand.ctx(), body.position(), &error)?))?;
        for _ in 0..dimension {
            let value = req_f64(&mut body)?;
            let Some(value) = FiniteReal::new(value) else {
                return Err(GeometryError::malformed(
                    body.position() - 8,
                    "nonfinite NURBS cage control value",
                ));
            };
            stored
                .push(expand.ctx(), value, "Rhino cage coordinate tuple")
                .or_else(|error| Err(refused(expand.ctx(), body.position(), &error)?))?;
        }
        let stored = stored
            .finish()
            .or_else(|error| Err(refused(expand.ctx(), body.position(), &error)?))?;
        let weight = if let Some(weights) = &mut weights {
            let weight = req_f64(&mut body)?;
            let Some(weight) = FiniteReal::new(weight) else {
                return Err(GeometryError::malformed(
                    body.position() - 8,
                    "nonfinite NURBS cage control value",
                ));
            };
            let weight = NonZeroReal::try_from(weight).map_err(|_| {
                GeometryError::malformed(body.position() - 8, "zero NURBS cage weight")
            })?;
            weights.push(weight);
            FiniteReal::from(weight)
        } else {
            FiniteReal::ONE
        };
        let mut point = expand
            .ctx()
            .collection_vec(dimension, "Rhino cage scaled coordinates")
            .map_err(crate::curves::GeometryError::from)?;
        for coordinate in stored {
            point.push(
                cadmpeg_ir::math::multiply_divide(coordinate, scale.real(), weight).ok_or_else(
                    || {
                        GeometryError::malformed(
                            body.position(),
                            "scaled NURBS cage coordinate is invalid",
                        )
                    },
                )?,
            );
        }
        control_points
            .push(expand.ctx(), point, "Rhino cage control points")
            .or_else(|error| Err(refused(expand.ctx(), body.position(), &error)?))?;
    }
    let remaining = body.remaining();
    body.skip(remaining).ok_or_else(|| {
        GeometryError::malformed(body.position(), "NURBS cage suffix is out of range")
    })?;
    let control_points = control_points
        .finish()
        .or_else(|error| Err(refused(expand.ctx(), body.position(), &error)?))?;
    Ok((
        Cage {
            source_range: offset..chunk.next_offset(),
            dimension,
            orders,
            counts,
            knots,
            control_points,
            weights,
        },
        chunk.next_offset(),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn cage_dimension_message_propagates_work_refusal() {
        let bytes = 0_i32.to_le_bytes();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, mut view) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("context");
        let error =
            super::positive(&ctx, &mut view, "dimension").expect_err("message work refuses");
        let GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) = error else {
            panic!("resource refusal");
        };
        assert_eq!(limit.operation, "Rhino positive text");
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }

    use super::{decode, ANONYMOUS};
    use crate::chunks::{ArchiveVersion, FramingError};
    use crate::curves::GeometryError;
    use crate::test_support::test_dump::crc_chunk;

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
            body.extend((f64::from(axis) + 1.0).to_le_bytes());
        }
        for index in 0..8 {
            let weight = if index == 7 { 2.0 } else { 1.0 };
            for coordinate in [f64::from(index) * weight, 0.0, 0.0, weight] {
                body.extend(coordinate.to_le_bytes());
            }
        }
        body
    }

    #[test]
    fn decodes_rational_cage_order_knots_and_u_v_w_control_order() {
        let bytes = crc_chunk(ArchiveVersion::V5, ANONYMOUS, &rational_cage_body());
        let cage = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(
                expand,
                0..bytes.len(),
                crate::test_support::millimeter_scale(10.0),
                ArchiveVersion::V8,
            )
        })
        .expect("required invariant");
        assert_eq!(cage.orders, [2, 2, 2]);
        assert_eq!(cage.counts, [2, 2, 2]);
        assert_eq!(
            cage.knots[2]
                .iter()
                .map(|knot| knot.get())
                .collect::<Vec<_>>(),
            [0.0, 3.0]
        );
        assert_eq!(
            cage.control_points[7]
                .iter()
                .map(|point| point.get())
                .collect::<Vec<_>>(),
            [70.0, 0.0, 0.0]
        );
        assert_eq!(
            cage.weights.as_ref().expect("required invariant")[7].get(),
            2.0
        );
    }

    fn cage_collection_refusal(limit: u64, operation: &str) {
        let bytes = crc_chunk(ArchiveVersion::V5, ANONYMOUS, &rational_cage_body());
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root bytes admitted");
        let expand = crate::mesh::MeshExpand::new(&ctx, root);
        let error = decode(
            expand,
            0..bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            ArchiveVersion::V8,
        )
        .expect_err("cage allocation exceeds collection limit");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.operation == operation
        ));
    }

    #[test]
    fn cage_knots_refuse_collection_limit() {
        cage_collection_refusal(1, "Rhino cage knot values");
    }

    #[test]
    fn cage_control_points_refuse_collection_limit() {
        cage_collection_refusal(13, "Rhino cage control points");
    }

    #[test]
    fn cage_weights_refuse_collection_limit() {
        cage_collection_refusal(21, "Rhino cage weights");
    }

    #[test]
    fn cage_coordinate_tuple_refuses_collection_limit() {
        cage_collection_refusal(24, "Rhino cage coordinate tuple");
    }

    #[test]
    fn cage_scaled_coordinates_refuse_collection_limit() {
        cage_collection_refusal(27, "Rhino cage scaled coordinates");
    }

    #[test]
    fn nonfinite_cage_knot_is_refused_at_source() {
        let mut body = rational_cage_body();
        body[40..48].copy_from_slice(&f64::INFINITY.to_le_bytes());
        let bytes = crc_chunk(ArchiveVersion::V5, ANONYMOUS, &body);
        let result = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(
                expand,
                0..bytes.len(),
                crate::test_support::millimeter_scale(10.0),
                ArchiveVersion::V8,
            )
        });
        assert!(matches!(
            result,
            Err(GeometryError::Malformed(FramingError::Structural { message, .. }))
                if message == "invalid NURBS cage knot"
        ));
    }

    #[test]
    fn zero_cage_weight_is_refused_at_source() {
        let mut body = rational_cage_body();
        body[112..120].copy_from_slice(&0.0_f64.to_le_bytes());
        let bytes = crc_chunk(ArchiveVersion::V5, ANONYMOUS, &body);
        let result = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(
                expand,
                0..bytes.len(),
                crate::test_support::millimeter_scale(10.0),
                ArchiveVersion::V8,
            )
        });
        assert!(matches!(
            result,
            Err(GeometryError::Malformed(FramingError::Structural { message, .. }))
                if message == "zero NURBS cage weight"
        ));
    }

    #[test]
    fn accepts_major_one_future_minor_and_skips_bounded_suffix() {
        let mut body = rational_cage_body();
        body[4..8].copy_from_slice(&2_i32.to_le_bytes());
        body.extend(0x1357_9bdf_i32.to_le_bytes());
        let bytes = crc_chunk(ArchiveVersion::V5, ANONYMOUS, &body);
        let cage = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(
                expand,
                0..bytes.len(),
                crate::test_support::millimeter_scale(10.0),
                ArchiveVersion::V8,
            )
        })
        .expect("major-one future minor is bounded-compatible");
        assert_eq!(cage.control_points[7][0].get(), 70.0);
    }

    #[test]
    fn rejects_a_non_one_major() {
        let mut body = rational_cage_body();
        body[..4].copy_from_slice(&2_i32.to_le_bytes());
        let bytes = crc_chunk(ArchiveVersion::V5, ANONYMOUS, &body);
        let result = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(
                expand,
                0..bytes.len(),
                crate::test_support::millimeter_scale(10.0),
                ArchiveVersion::V8,
            )
        });
        assert!(matches!(
            result,
            Err(GeometryError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn truncating_the_control_net_is_rejected_at_the_record_boundary() {
        // Drop the final control-point tuple so the count-framed control loop
        // runs past the record body's proven window.
        let mut body = rational_cage_body();
        body.truncate(body.len() - 32);
        let bytes = crc_chunk(ArchiveVersion::V5, ANONYMOUS, &body);
        assert!(crate::decode::with_expand_bytes(&bytes, |expand| decode(
            expand,
            0..bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            ArchiveVersion::V8
        ))
        .is_err());
    }
    #[test]
    fn audit_regression_cage_unweighting_retains_finite_scaled_poles() {
        let mut body = rational_cage_body();
        let start = body.len() - 8 * 4 * 8;
        for pole in body[start..].chunks_exact_mut(32) {
            pole[..8].copy_from_slice(&1e300_f64.to_le_bytes());
            pole[24..].copy_from_slice(&1e-10_f64.to_le_bytes());
        }
        let bytes = crc_chunk(ArchiveVersion::V5, ANONYMOUS, &body);
        let cage = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(
                expand,
                0..bytes.len(),
                crate::test_support::millimeter_scale(1e-7),
                ArchiveVersion::V8,
            )
        })
        .unwrap();
        for point in &cage.control_points {
            assert!((point[0].get() / 1e303 - 1.).abs() <= 8. * f64::EPSILON);
        }
    }
}
