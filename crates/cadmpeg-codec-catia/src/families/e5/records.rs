//! E5 storage-variant record decoders.
//!
//! Decodes E5 `05 08 01` vertex rosters, inline `0xc9` circle carriers,
//! class-`0xc8` planes, `0xff` edge-use records, and cylinder/cone/torus
//! analytic surface carriers.

use crate::families::a5a8::records::rolling_ball_jet_derivative;
use crate::math::distance;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::{
    nurbs::NurbsSurface, CurveGeometry, ProceduralSurfaceDefinition, RollingBallJetSite,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal, PositiveAngle, PositiveLength};
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

use crate::families::e5::graph::Sign;
use crate::wire::bytes::{f64_le, f64_point, f64_vector, read_f64_array, u32_le_24};
use crate::wire::records::scan_vertex_records;

/// A directly decoded E5 circle carrier.
#[derive(Debug, Clone)]
pub(in crate::families::e5) struct E5Circle {
    /// Offset of the `e5 0d 03` record in the source buffer.
    pub(super) pos: usize,
    /// The complete circle carrier.
    pub(super) geometry: CurveGeometry,
}

/// Partial class-`0xc8` plane carrier containing the fields stored directly.
#[derive(Debug, Clone)]
pub(in crate::families::e5) struct E5Plane {
    /// Offset of the framed record.
    pub(super) pos: usize,
    /// Stream-assigned record identifier.
    pub(super) record_id: u32,
    /// Stored plane origin.
    pub(super) origin: FinitePoint3,
    /// Natural U-coordinate bounds.
    #[cfg(test)]
    pub(super) u_range: [f64; 2],
    /// Natural V-coordinate bounds.
    #[cfg(test)]
    pub(super) v_range: [f64; 2],
}

/// A directly decoded E5 analytic surface carrier.
#[derive(Debug, Clone)]
pub(in crate::families) struct E5Surface {
    /// Offset of the `e5 0d 03` record in the source buffer.
    pub(super) pos: usize,
    /// Persistent E5 record id.
    pub(in crate::families) record_id: u32,
    /// The complete analytic surface carrier.
    pub(in crate::families) geometry: SurfaceGeometry,
    /// Component-wise scale from native E5 UV coordinates to neutral UV.
    pub(super) uv_scale: [FiniteReal; 2],
}

/// A class-`0xd8` E5 rolling-ball surface carrier.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct E5RollingBallJet {
    /// Offset of the framed record.
    pub(super) pos: usize,
    /// Persistent E5 record id.
    pub(in crate::families) record_id: u32,
    /// Knots, multiplicities, and complete derivative channels in native order.
    stations:
        Vec<cadmpeg_ir::geometry::RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3>>,
    /// Native surface-sense flag retained without reinterpretation.
    pub(in crate::families) sense: Sign,
}

impl E5RollingBallJet {
    /// Degree of every scalar jet channel.
    const DEGREE: u32 = 5;

    /// Convert the admitted carrier payload to the exact neutral jet form.
    pub(in crate::families) fn definition(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<ProceduralSurfaceDefinition>, CodecError> {
        let stations = crate::resource::copy_retained_slice(ctx, &self.stations, "catia_e5_rolling_ball_definition_stations")?;
        Ok(cadmpeg_ir::geometry::RollingBallJetStations::from_admitted(
            Self::DEGREE,
            stations,
        )
        .ok()
        .map(ProceduralSurfaceDefinition::RollingBallJet))
    }
}

/// A class-`0xf1` surface wrapper.
///
/// The wrapper keeps the five serialized references. Its first reference is
/// the underlying surface carrier when the wrapper is used by an E5 face;
/// the remaining references and tail are retained structurally by the frame
/// but are not assigned a separate meaning here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::families) struct E5SurfaceWrapper {
    /// Offset of the framed record.
    pos: usize,
    /// Persistent E5 record id.
    pub(in crate::families) record_id: u32,
    /// Five references in serialized order.
    references: [u32; 5],
}

impl E5SurfaceWrapper {
    /// The first reference, which names the wrapped geometric carrier.
    #[must_use]
    pub(in crate::families) fn underlying_surface(&self) -> u32 {
        self.references[0]
    }
}

#[derive(Clone, Copy)]
struct E5Record {
    pos: usize,
    class: u8,
    size: usize,
}

impl E5Record {
    fn end(&self) -> usize {
        self.pos + self.size + 13
    }
}

/// The E5 record-family token from the layout table.
///
/// The container scanner finds E5 records with
/// [`crate::container::E5_MARKER`]. The two constants state the same three
/// bytes; the assertion below fails the build if they diverge.
const MARKER: &[u8; 3] = &crate::layout::token::E5_RECORD_FAMILY;

const _: () = assert!(
    MARKER[0] == crate::container::E5_MARKER[0]
        && MARKER[1] == crate::container::E5_MARKER[1]
        && MARKER[2] == crate::container::E5_MARKER[2],
    "the E5 layout token and the container E5 marker must state the same bytes"
);

const E5_NURBS_SURFACE_TAIL_BYTES: usize = 148;
const E5_D8_TAIL_BYTES: usize = 63;
const E5_D8_ARC_TOLERANCE: f64 = 1e-8;
const E5_D8_RADIUS_TOLERANCE: f64 = 1e-8;

fn e5_records(data: &[u8]) -> impl Iterator<Item = E5Record> + '_ {
    crate::container::all_e5_record_spans(data)
        .filter_map(|range| {
            let pos = range.start;
            let size = View::u16_le_at(data, pos + 5).map(usize::from)?;
            Some(E5Record {
                pos,
                class: data[pos + 3],
                size,
            })
        })
}

/// Read the complete ordered E5 `05 08 01` coordinate roster matching the
/// referenced vertex population. The roster may be split into multiple runs;
/// marker-like bytes inside framed payloads are not vertex rows.
pub(super) fn e5_vertices(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    vertex_count: usize,
) -> Result<Vec<FinitePoint3>, CodecError> {
    if vertex_count == 0 {
        return Ok(Vec::new());
    }
    let mut vertices = Vec::new();
    let mut region_start = 0usize;
    for record in e5_records(data) {
        for vertex in scan_vertex_records(&data[region_start..record.pos]) {
            crate::resource::push(ctx, &mut vertices, vertex, "catia_e5_vertex_roster")?;
        }
        region_start = record.end();
    }
    for vertex in scan_vertex_records(&data[region_start..]) {
        crate::resource::push(ctx, &mut vertices, vertex, "catia_e5_vertex_roster")?;
    }
    if vertices.len() != vertex_count {
        return Ok(Vec::new());
    }
    Ok(vertices)
}

/// Walk an E5 record stream and decode its inline `0xc9` circle carriers.
/// Record strides are derived from the little-endian size field at `+5`.
pub(super) fn e5_circles(ctx: &DecodeContext<'_>, data: &[u8]) -> Result<Vec<E5Circle>, CodecError> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        if record.class == 0xc9 && record.size >= 81 {
            let origin = f64_point(data, pos + 14);
            let frame_u = f64_vector(data, pos + 38);
            let frame_v = f64_vector(data, pos + 62);
            let radius = f64_le(data, pos + 86);
            if let (Some(origin), Some(frame_u), Some(frame_v), Some(radius)) =
                (origin, frame_u, frame_v, radius)
            {
                let (frame_u, frame_v) = (frame_u.get(), frame_v.get());
                if let Some(radius) = PositiveLength::new(radius.get()) {
                    if let Some(axis) = UnitVector3::normalized(frame_u.cross(frame_v)) {
                        let reference = UnitVector3::normalized(frame_u).or_else(|| {
                            UnitVector3::new(cadmpeg_ir::geometry::derive_reference_direction(
                                *axis.as_raw(),
                            ))
                        });
                        let Some(frame) = reference
                            .and_then(|reference| OrthonormalFrame3::from_units(axis, reference))
                        else {
                            continue;
                        };
                        let payload =
                            cadmpeg_ir::geometry::analytic::CircleCurve::new(origin, frame, radius);
                        crate::resource::push(ctx, &mut out, E5Circle {
                            pos,
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(payload)),
                        }, "catia_e5_circles")?;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Decode the byte-explicit origin and natural bounds of E5 class-`0xc8` planes.
///
/// The record does not store a complete in-plane frame, so this function does not
/// synthesize plane axes or a [`SurfaceGeometry`].
#[must_use]
pub(super) fn e5_planes(ctx: &DecodeContext<'_>, data: &[u8]) -> Result<Vec<E5Plane>, CodecError> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        if record.class != 0xc8 || record.size < 90 || (record.size - 90) % 8 != 0 {
            continue;
        }
        let Some(origin) = f64_point(data, pos + 14) else {
            continue;
        };
        let scalar_count = (record.size - 58) / 8;
        let scalars_finite =
            (0..scalar_count).all(|index| f64_le(data, pos + 39 + 8 * index).is_some());
        let Some(bounds) = read_f64_array::<4>(data, record.end() - 32) else {
            continue;
        };
        if !scalars_finite {
            continue;
        }
        #[cfg(not(test))]
        // discarded-value: reading the natural bounds admits them finite; only tests read them
        let _ = bounds;
        crate::resource::push(ctx, &mut out, E5Plane {
            pos,
            record_id: View::u32_le_at(data, pos + 9).unwrap_or(0),
            origin,
            #[cfg(test)]
            u_range: [bounds[0].get(), bounds[1].get()],
            #[cfg(test)]
            v_range: [bounds[2].get(), bounds[3].get()],
        }, "catia_e5_planes")?;
    }
    Ok(out)
}

/// A directly framed E5 edge-use record.  The endpoint ids are E5 vertex
/// records, not point-table indexes.
#[derive(Debug, Clone)]
pub(in crate::families::e5) struct E5Edge {
    /// Referenced start-vertex (class `0xfe`) record id.
    pub(super) start_vertex_id: u32,
    /// Referenced end-vertex (class `0xfe`) record id.
    pub(super) end_vertex_id: u32,
}

/// Decode E5 `0xff` five-reference edge records.
pub(super) fn e5_edges(ctx: &DecodeContext<'_>, data: &[u8]) -> Result<Vec<E5Edge>, CodecError> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        if record.class == 0xff && data.get(pos + 13) == Some(&0x85) {
            let payload = &data[pos + 13..record.end()];
            if let Some((_, next)) = e5_ref(payload, 1) {
                if let Some((start_vertex_id, next)) = e5_ref(payload, next) {
                    if let Some((end_vertex_id, _)) = e5_ref(payload, next) {
                        crate::resource::push(ctx, &mut out, E5Edge {
                            start_vertex_id,
                            end_vertex_id,
                        }, "catia_e5_edges")?;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Decode E5 cylinder (`0xc9`), cone (`0xca`), and torus (`0xcc`) surface
/// records. The E5 plane class does not serialize a standalone normal.
pub(in crate::families) fn e5_surfaces(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<E5Surface>, CodecError> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        let decoded = match record.class {
            0xc9 => e5_cylinder(data, pos).and_then(|(geometry, radius)| {
                Some((
                    geometry,
                    [FiniteReal::new(1.0 / radius.get())?, FiniteReal::ONE],
                ))
            }),
            0xca => e5_cone(data, pos).and_then(|(geometry, half_angle)| {
                let u_scale = f64_le(data, pos + 158)?.get();
                let v_scale = f64_le(data, pos + 166)?.get();
                if u_scale == 0.0 || v_scale == 0.0 {
                    return None;
                }
                Some((
                    geometry,
                    [
                        FiniteReal::new(1.0 / u_scale)?,
                        FiniteReal::new(half_angle.get().cos() / v_scale)?,
                    ],
                ))
            }),
            0xcc => e5_torus(data, pos).and_then(|(geometry, major_radius, minor_radius)| {
                Some((
                    geometry,
                    [
                        FiniteReal::new(1.0 / major_radius.get())?,
                        FiniteReal::new(1.0 / minor_radius.get())?,
                    ],
                ))
            }),
            0xe7 => e5_nurbs_surface(ctx, data, record, refusal)?
                .map(|geometry| (geometry, [FiniteReal::ONE, FiniteReal::ONE])),
            _ => None,
        };
        if let Some((geometry, uv_scale)) = decoded {
            crate::resource::push(ctx, &mut out, E5Surface {
                pos,
                record_id: View::u32_le_at(data, pos + 9).unwrap_or(0),
                geometry,
                uv_scale,
            }, "catia_e5_surfaces")?;
        }
    }
    Ok(out)
}

/// Decode E5 class-`0xd8` rolling-ball surface carriers.
///
/// The carrier stores three structure-of-arrays lanes of ten f64 channels:
/// position, first derivative, and second derivative. The first six channels
/// are the two limiting points, the next three are the centre, and the last
/// channel is the opening angle. The tail range, radius, and sense are kept on
/// the native record; the neutral definition contains the complete value and
/// derivative jets.
pub(in crate::families) fn e5_rolling_ball_jets(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<E5RollingBallJet>, CodecError> {
    let mut jets = Vec::new();
    for record in e5_records(data)
        .into_iter()
        .filter(|record| record.class == 0xd8)
    {
        if let Some(jet) = parse_e5_rolling_ball_jet(ctx, data, record)? {
            crate::resource::push(ctx, &mut jets, jet, "catia_e5_rolling_ball_jets")?;
        }
    }
    Ok(jets)
}

fn parse_e5_rolling_ball_jet(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    record: E5Record,
) -> Result<Option<E5RollingBallJet>, CodecError> {
    let Some((mut view, station_count)) = (|| {
        let mut view =
            View::over_retained(data).child(record.pos.checked_add(13)?, record.end())?;
        if view.u8()? != 0x80 {
            return None;
        }
        let station_count = usize::try_from(view.u32_le()?).ok()?;
        let degree = view.u32_le()?;
        let zero0 = view.u32_le()?;
        let zero1 = view.u32_le()?;
        let repeated_station_count = usize::try_from(view.u32_le()?).ok()?;
        let zero2 = view.u32_le()?;
        (station_count >= 2
            && degree == E5RollingBallJet::DEGREE
            && repeated_station_count == station_count
            && [zero0, zero1, zero2] == [0; 3]
            && record.size
                == station_count
                    .checked_mul(252)
                    .and_then(|size| size.checked_add(88))?)
        .then_some((view, station_count))
    })() else {
        return Ok(None);
    };
    // Knots, multiplicities, three channel lanes, sites, and stations each
    // contain one item per declared station.
    let station_count_u64 = station_count as u64;
    ctx.charge_collection_items(
        station_count_u64 * 7,
        "decode CATIA E5 rolling-ball stations",
    )?;
    Ok((|| {
        let knots =
            view.read_counted(station_count_u64, 8, |view| FiniteReal::new(view.f64_le()?))?;
        if knots.windows(2).any(|pair| pair[0] >= pair[1]) {
            return None;
        }
        let multiplicities = view.read_counted(station_count_u64, 4, View::u32_le)?;
        // `station_count < 2` is refused above, so the interior station count is
        // the exact difference. The checked subtraction refuses a stated count this
        // record cannot span instead of saturating it to an empty interior, which
        // would admit any interior multiplicity.
        let interior_station_count = station_count.checked_sub(2)?;
        if multiplicities.first() != Some(&6)
            || multiplicities.last() != Some(&6)
            || multiplicities
                .iter()
                .skip(1)
                .take(interior_station_count)
                .any(|multiplicity| *multiplicity != 3)
        {
            return None;
        }
        let positions = read_d8_channel_rows(&mut view, station_count_u64)?;
        let first_derivatives = read_d8_channel_rows(&mut view, station_count_u64)?;
        let second_derivatives = read_d8_channel_rows(&mut view, station_count_u64)?;
        if view.remaining() != E5_D8_TAIL_BYTES {
            return None;
        }
        let parameter_min = view.f64_le()?;
        let parameter_max = view.f64_le()?;
        let tail_zero0 = view.f64_le()?;
        let tail_radius0 = view.f64_le()?;
        let tail_radius1 = view.f64_le()?;
        let sense = match view.i32_le()? {
            -1 => Sign::Negative,
            1 => Sign::Positive,
            _ => return None,
        };
        let tail_zero1 = view.f64_le()?;
        let tail_radius2 = view.f64_le()?;
        if parameter_min.to_bits() != knots.first()?.get().to_bits()
            || parameter_max.to_bits() != knots.last()?.get().to_bits()
            || tail_zero0.to_bits() != 0
            || tail_zero1.to_bits() != 0
            || !tail_radius0.is_finite()
            || tail_radius0 <= 0.0
            || !relative_close(tail_radius0, tail_radius1, E5_D8_RADIUS_TOLERANCE)
            || !relative_close(tail_radius0, tail_radius2, E5_D8_RADIUS_TOLERANCE)
            || view.array::<3>()? != [1, 0, 0]
        {
            return None;
        }
        let sites = positions
            .into_iter()
            .zip(first_derivatives)
            .zip(second_derivatives)
            .map(|((position, first), second)| {
                let first_limit =
                    FinitePoint3::from_coordinates(position[0], position[1], position[2]);
                let second_limit =
                    FinitePoint3::from_coordinates(position[3], position[4], position[5]);
                let center = FinitePoint3::from_coordinates(position[6], position[7], position[8]);
                let radius = distance(center.get(), first_limit.get());
                let second_radius = distance(center.get(), second_limit.get());
                let expected_angle = if radius > 0.0 && second_radius > 0.0 {
                    first_limit
                        .get()
                        .vector_from(center.get())
                        .scale(1.0 / radius)
                        .dot(
                            second_limit
                                .get()
                                .vector_from(center.get())
                                .scale(1.0 / second_radius),
                        )
                        .clamp(-1.0, 1.0)
                        .acos()
                } else {
                    f64::NAN
                };
                (
                    first_limit,
                    second_limit,
                    center,
                    radius,
                    second_radius,
                    expected_angle,
                    position[9],
                    first,
                    second,
                )
            })
            .collect::<Vec<_>>();
        if sites.iter().any(
            |(
                _first_limit,
                _second_limit,
                _center,
                radius,
                second_radius,
                expected_angle,
                stored_angle,
                _first,
                _second,
            )| {
                !radius.is_finite()
                    || *radius <= 0.0
                    || !second_radius.is_finite()
                    || !relative_close(*radius, *second_radius, E5_D8_RADIUS_TOLERANCE)
                    || !expected_angle.is_finite()
                    || (stored_angle.get() - expected_angle).abs() > E5_D8_ARC_TOLERANCE
                    || !relative_close(*radius, tail_radius0, E5_D8_RADIUS_TOLERANCE)
            },
        ) {
            return None;
        }
        let stations = sites
            .into_iter()
            .map(
                |(
                    first_limit,
                    second_limit,
                    center,
                    _radius,
                    _second_radius,
                    _expected_angle,
                    angle,
                    first,
                    second,
                )| RollingBallJetSite {
                    first_limit,
                    second_limit,
                    center,
                    angle,
                    first_derivative: rolling_ball_jet_derivative(first),
                    second_derivative: rolling_ball_jet_derivative(second),
                },
            )
            .zip(knots)
            .zip(multiplicities)
            .map(
                |((site, knot), multiplicity)| cadmpeg_ir::geometry::RollingBallJetStation {
                    knot,
                    multiplicity,
                    site,
                },
            )
            .collect();
        Some(E5RollingBallJet {
            pos: record.pos,
            record_id: View::u32_le_at(data, record.pos + 9)?,
            stations,
            sense,
        })
    })())
}

fn read_d8_channel_rows(
    view: &mut View<'_>,
    station_count_u64: u64,
) -> Option<Vec<[FiniteReal; 10]>> {
    view.read_counted(station_count_u64, 80, |view| {
        let mut row = [FiniteReal::ZERO; 10];
        for value in &mut row {
            *value = FiniteReal::new(view.f64_le()?)?;
        }
        Some(row)
    })
}

fn relative_close(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() <= tolerance * left.abs().max(right.abs()).max(1.0)
}

/// Decode fixed-size class-`0xf1` surface wrappers.
///
/// The admitted grammar is a five-reference lane (`85` plus five restricted
/// reference tokens) followed by the wrapper tail, with the complete payload
/// fixed at 44 bytes. Reference tokens retain their encoded widths; the tail
/// remains opaque. Only the exact frame and reference lane are needed to join
/// a face wrapper to a directly decoded geometric carrier.
#[must_use]
pub(in crate::families) fn e5_surface_wrappers(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<E5SurfaceWrapper>, CodecError> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        if record.class != 0xf1 || record.size != 44 {
            continue;
        }
        let payload = &data[record.pos + 13..record.end()];
        if payload.first() != Some(&0x85) {
            continue;
        }
        let mut references = [0u32; 5];
        let mut next = 1;
        let mut complete = true;
        for reference in &mut references {
            let Some(value) = crate::wire::tokens::object_ref(payload, &mut next, false) else {
                complete = false;
                break;
            };
            *reference = value;
        }
        if !complete {
            continue;
        }
        if next >= 44 {
            continue;
        }
        crate::resource::push(ctx, &mut out, E5SurfaceWrapper {
            pos: record.pos,
            record_id: View::u32_le_at(data, record.pos + 9).unwrap_or(0),
            references,
        }, "catia_e5_surface_wrappers")?;
    }
    Ok(out)
}

fn e5_nurbs_surface(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    record: E5Record,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<SurfaceGeometry>, CodecError> {
    let Some(mut view) = View::over_retained(data).child(record.pos + 13, record.end()) else {
        return Ok(None);
    };
    if view.u8() != Some(0x80) {
        return Ok(None);
    }
    let Some((u_degree, u_knots, u_multiplicities)) = read_nurbs_axis(ctx, &mut view)? else {
        return Ok(None);
    };
    let Some((v_degree, v_knots, v_multiplicities)) = read_nurbs_axis(ctx, &mut view)? else {
        return Ok(None);
    };
    let Some((u_knots, u_count)) = expand_nurbs_axis(ctx, u_degree, &u_knots, &u_multiplicities, record.size)? else {
        return Ok(None);
    };
    let Some((v_knots, v_count)) = expand_nurbs_axis(ctx, v_degree, &v_knots, &v_multiplicities, record.size)? else {
        return Ok(None);
    };
    let Some(mode) = view.u16_le() else { return Ok(None); };
    if !matches!(mode, 0 | 1) {
        return Ok(None);
    }
    let Some(control_count) = u_count.checked_mul(v_count) else { return Ok(None); };
    let Some(control_count_u64) = u64::try_from(control_count).ok() else { return Ok(None); };
    let point_bytes = if mode == 1 { 32 } else { 24 };
    if view.counted(control_count_u64, point_bytes).is_none() {
        return Ok(None);
    }
    let Some(retained_bytes) = control_count_u64.checked_mul(24) else {
        return Err(ctx.refuse_codec_limit("catia_e5_nurbs_poles", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(retained_bytes, "catia_e5_nurbs_poles")?;
    let mut control_points = Vec::new();
    crate::resource::reserve_vec(ctx, &mut control_points, control_count, "catia_e5_nurbs_control_points")?;
    for _ in 0..control_count {
        let Some(point) = (|| FinitePoint3::new(Point3::new(view.f64_le()?, view.f64_le()?, view.f64_le()?)))() else {
            return Ok(None);
        };
        control_points.push(point);
    }
    let weights = if mode == 1 {
        let Some(bytes) = control_count_u64.checked_mul(8) else {
            return Err(ctx.refuse_codec_limit("catia_e5_nurbs_weights", u64::MAX, u64::MAX));
        };
        ctx.charge_retained(bytes, "catia_e5_nurbs_weights")?;
        let mut weights = Vec::new();
        crate::resource::reserve_vec(ctx, &mut weights, control_count, "catia_e5_nurbs_weights")?;
        for _ in 0..control_count {
            let Some(weight) = view.f64_le().and_then(NonZeroReal::new) else { return Ok(None); };
            weights.push(weight);
        }
        Some(weights)
    } else {
        None
    };
    if view.remaining() != E5_NURBS_SURFACE_TAIL_BYTES {
        return Ok(None);
    }
    if view.skip(E5_NURBS_SURFACE_TAIL_BYTES).is_none() || !view.is_empty() {
        return Ok(None);
    }
    let mut point_rows = Vec::new();
    for row in control_points.chunks(v_count) {
        let copied = crate::resource::copy_retained_slice(ctx, row, "catia_e5_nurbs_point_row")?;
        crate::resource::push(ctx, &mut point_rows, copied, "catia_e5_nurbs_point_rows")?;
    }
    let weight_rows = if let Some(weights) = weights {
        let mut rows = Vec::new();
        for row in weights.chunks(v_count) {
            let copied = crate::resource::copy_retained_slice(ctx, row, "catia_e5_nurbs_weight_row")?;
            crate::resource::push(ctx, &mut rows, copied, "catia_e5_nurbs_weight_rows")?;
        }
        Some(rows)
    } else {
        None
    };
    let poles = if let Some(weight_rows) = weight_rows {
        let Some(bytes) = control_count.checked_mul(size_of::<cadmpeg_ir::geometry::nurbs::WeightedPole3<FinitePoint3>>()).and_then(|bytes| u64::try_from(bytes).ok()) else {
            return Err(ctx.refuse_codec_limit("catia_e5_nurbs_weighted_poles", u64::MAX, u64::MAX));
        };
        ctx.charge_retained(bytes, "catia_e5_nurbs_weighted_poles")?;
        let mut rows = Vec::new();
        for (points, weights) in point_rows.into_iter().zip(weight_rows) {
            let mut row = Vec::new();
            crate::resource::reserve_vec(ctx, &mut row, points.len(), "catia_e5_nurbs_weighted_poles")?;
            row.extend(points.into_iter().zip(weights).map(|(point, weight)| cadmpeg_ir::geometry::nurbs::WeightedPole3 { point, weight }));
            crate::resource::push(ctx, &mut rows, row, "catia_e5_nurbs_weighted_rows")?;
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Rational { rows }
    } else {
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Polynomial { rows: point_rows }
    };
    Ok(crate::nurbs::note_refusal(
        NurbsSurface::from_admitted(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(u_degree, u_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(v_degree, v_knots, false),
            poles,
            false,
        ),
        refusal,
        format_args!("e5 NURBS surface record at byte {}", record.pos),
    )
    .map(SolvedSurfaceGeometry::Nurbs)
    .map(SurfaceGeometry::Solved))
}

fn read_nurbs_axis(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
) -> Result<Option<(u32, Vec<f64>, Vec<u32>)>, CodecError> {
    let Some((degree, zero0, zero1, knot_count, zero2)) = (|| {
        Some((view.u32_le()?, view.u32_le()?, view.u32_le()?, usize::try_from(view.u32_le()?).ok()?, view.u32_le()?))
    })() else {
        return Ok(None);
    };
    if degree == 0 || [zero0, zero1, zero2] != [0; 3] || knot_count == 0 {
        return Ok(None);
    }
    let Some(knot_count_u64) = u64::try_from(knot_count).ok() else {
        return Ok(None);
    };
    if view.counted(knot_count_u64, 12).is_none() {
        return Ok(None);
    }
    let Some(bytes) = knot_count_u64.checked_mul(8) else {
        return Err(ctx.refuse_codec_limit("catia_e5_nurbs_axis", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, "catia_e5_nurbs_axis")?;
    let mut knots = Vec::new();
    crate::resource::reserve_vec(ctx, &mut knots, knot_count, "catia_e5_nurbs_axis_knots")?;
    for _ in 0..knot_count {
        let Some(knot) = view.f64_le() else { return Ok(None); };
        knots.push(knot);
    }
    let Some(bytes) = knot_count_u64.checked_mul(4) else {
        return Err(ctx.refuse_codec_limit("catia_e5_nurbs_axis", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, "catia_e5_nurbs_axis")?;
    let mut multiplicities = Vec::new();
    crate::resource::reserve_vec(ctx, &mut multiplicities, knot_count, "catia_e5_nurbs_axis_multiplicities")?;
    for _ in 0..knot_count {
        let Some(multiplicity) = view.u32_le() else { return Ok(None); };
        multiplicities.push(multiplicity);
    }
    Ok(Some((degree, knots, multiplicities)))
}

fn expand_nurbs_axis(
    ctx: &DecodeContext<'_>,
    degree: u32,
    knots: &[f64],
    multiplicities: &[u32],
    payload_size: usize,
) -> Result<Option<(Vec<f64>, usize)>, CodecError> {
    if knots.len() != multiplicities.len()
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities.contains(&0)
    {
        return Ok(None);
    }
    let Some(total) = multiplicities
        .iter()
        .try_fold(0usize, |total, multiplicity| {
            total.checked_add(usize::try_from(*multiplicity).ok()?)
        }) else { return Ok(None); };
    if total > payload_size {
        return Ok(None);
    }
    let Some((degree, control_count)) = usize::try_from(degree).ok().and_then(|degree| Some((degree, total.checked_sub(degree.checked_add(1)?)?))) else {
        return Ok(None);
    };
    if control_count <= degree || knots.first() >= knots.last() {
        return Ok(None);
    }
    let Some(bytes) = total.checked_mul(size_of::<f64>()).and_then(|bytes| u64::try_from(bytes).ok()) else {
        return Err(ctx.refuse_codec_limit("catia_e5_nurbs_expanded_axis", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, "catia_e5_nurbs_expanded_axis")?;
    let mut expanded = Vec::new();
    crate::resource::reserve_vec(ctx, &mut expanded, total, "catia_e5_nurbs_expanded_axis")?;
    for (knot, multiplicity) in knots.iter().zip(multiplicities) {
        let Ok(count) = usize::try_from(*multiplicity) else { return Ok(None); };
        expanded.extend(std::iter::repeat_n(*knot, count));
    }
    Ok((expanded.len() == total).then_some((expanded, control_count)))
}

fn e5_cylinder(data: &[u8], pos: usize) -> Option<(SurfaceGeometry, PositiveLength)> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14)?;
    let origin = c.point3()?;
    let (geometry, radius) = crate::analytic::cylinder_uvr(&mut c, origin)?;
    Some((geometry, radius))
}

fn e5_cone(data: &[u8], pos: usize) -> Option<(SurfaceGeometry, PositiveAngle)> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14)?;
    let (geometry, _, half_angle) = crate::analytic::cone_ozra(&mut c)?;
    Some((geometry, half_angle))
}

fn e5_torus(data: &[u8], pos: usize) -> Option<(SurfaceGeometry, PositiveLength, PositiveLength)> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14)?;
    let (geometry, major_radius, minor_radius) = crate::analytic::torus_ozrr(&mut c)?;
    Some((geometry, major_radius, minor_radius))
}

fn e5_ref(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    match *bytes.get(at)? {
        0x38 => Some((u32_le_24(bytes, at + 1)?, at + 4)),
        0x18 => Some((View::u16_le_at(bytes, at + 1)? as u32, at + 3)),
        0x10 => Some((u32::from(*bytes.get(at + 1)?) << 8, at + 2)),
        0x08 => Some((*bytes.get(at + 1)? as u32, at + 2)),
        byte if byte >= 0x80 => Some(((byte - 0x80) as u32, at + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    use super::{
        e5_cone, e5_ref, e5_rolling_ball_jets, e5_surface_wrappers, e5_surfaces, e5_torus,
    };
    use crate::test_support::test_e5::append_e5_record;

    const TEST_F64_TOLERANCE: f64 = 1e-12;

    fn cone_record() -> Vec<u8> {
        let mut bytes = crate::test_support::test_e5::e5_torus_stream();
        bytes[3] = 0xca;
        bytes[110..118].copy_from_slice(&std::f64::consts::FRAC_PI_4.to_le_bytes());
        bytes[118..126].copy_from_slice(&2.0_f64.to_le_bytes());
        bytes
    }

    #[test]
    fn e5_cone_admits_positive_radius_and_acute_half_angle() {
        let bytes = cone_record();
        assert!(
            matches!(e5_cone(&bytes, 0), Some((_, angle)) if angle.get() == std::f64::consts::FRAC_PI_4)
        );
    }

    #[test]
    fn e5_cone_refuses_zero_radius() {
        let mut bytes = cone_record();
        bytes[118..126].copy_from_slice(&0.0_f64.to_le_bytes());
        assert!(e5_cone(&bytes, 0).is_none());
    }

    #[test]
    fn e5_cone_refuses_nonpositive_half_angle() {
        let mut bytes = cone_record();
        bytes[110..118].copy_from_slice(&std::f64::consts::FRAC_PI_2.to_le_bytes());
        assert!(e5_cone(&bytes, 0).is_none());
    }

    #[test]
    fn e5_cone_refuses_half_angle_at_or_above_half_pi() {
        let mut bytes = cone_record();
        bytes[110..118].copy_from_slice(&0.0_f64.to_le_bytes());
        assert!(e5_cone(&bytes, 0).is_none());
    }

    #[test]
    fn e5_torus_refuses_zero_minor_radius() {
        let mut bytes = crate::test_support::test_e5::e5_torus_stream();
        bytes[118..126].copy_from_slice(&0.0_f64.to_le_bytes());
        assert!(e5_torus(&bytes, 0).is_none());
    }

    #[test]
    fn e5_torus_refuses_negative_minor_radius() {
        let mut bytes = crate::test_support::test_e5::e5_torus_stream();
        bytes[118..126].copy_from_slice(&(-2.0_f64).to_le_bytes());
        assert!(e5_torus(&bytes, 0).is_none());
    }

    fn decoded_jets(bytes: &[u8]) -> Result<Vec<super::E5RollingBallJet>, CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())?;
        e5_rolling_ball_jets(&ctx, bytes)
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() <= TEST_F64_TOLERANCE);
    }

    fn assert_point_close(actual: Point3, expected: Point3) {
        assert_close(actual.x, expected.x);
        assert_close(actual.y, expected.y);
        assert_close(actual.z, expected.z);
    }

    fn assert_vector_close(actual: Vector3, expected: Vector3) {
        assert_close(actual.x, expected.x);
        assert_close(actual.y, expected.y);
        assert_close(actual.z, expected.z);
    }

    fn append_nurbs_axis(payload: &mut Vec<u8>, degree: u32) {
        payload.extend_from_slice(&degree.to_le_bytes());
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[0.0_f64.to_le_bytes(), 1.0_f64.to_le_bytes()].concat());
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.extend_from_slice(&2_u32.to_le_bytes());
    }

    #[test]
    fn d8_record_decodes_the_quintic_rolling_ball_jet() {
        let bytes = crate::test_support::test_e5::e5_d8_rolling_ball_stream();

        let jets = decoded_jets(&bytes).expect("service profile admits two stations");
        assert_eq!(jets.len(), 1);
        let jet = &jets[0];
        assert_eq!(jet.record_id, 42);
        assert_eq!(jet.stations.len(), 2);
        assert_close(jet.stations[0].knot.get(), 2.0);
        assert_close(jet.stations[1].knot.get(), 5.0);
        assert_eq!(
            jet.stations
                .iter()
                .map(|station| station.multiplicity)
                .collect::<Vec<_>>(),
            [6, 6]
        );
        assert_eq!(jet.sense, crate::families::e5::graph::Sign::Negative);
        assert_point_close(
            jet.stations[0].site.first_limit.get(),
            Point3::new(2.0, 0.0, 0.0),
        );
        assert_point_close(
            jet.stations[1].site.center.get(),
            Point3::new(1.0, 0.0, 0.0),
        );
        assert_close(
            jet.stations[0].site.angle.get(),
            std::f64::consts::FRAC_PI_2,
        );
        assert_vector_close(
            jet.stations[0].site.first_derivative.center.get(),
            Vector3::new(0.7, 0.8, 0.9),
        );
        assert_vector_close(
            jet.stations[0].site.second_derivative.center.get(),
            Vector3::new(2.7, 2.8, 2.9),
        );
        assert_close(jet.stations[1].site.second_derivative.angle.get(), 4.0);
        assert!(matches!(
            crate::test_support::with_service_context(|ctx| jet.definition(ctx))
                .expect("service resource budget")
                .expect("valid rolling-ball jet fixture"),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet(jet) if jet.degree() == 5 && jet.stations().len() == 2 && jet.stations().iter().map(|station| station.multiplicity).collect::<Vec<_>>() == [6, 6]));
    }

    #[test]
    fn d8_record_rejects_wrong_tail_marker() {
        let mut payload = crate::test_support::test_e5::e5_d8_rolling_ball_stream();
        let last = payload.len() - 3;
        payload[last] = 0;
        assert!(decoded_jets(&payload)
            .expect("service profile admits two stations")
            .is_empty());
    }

    #[test]
    fn e5_station_collection_limit_refuses_before_station_vectors() {
        let bytes = crate::test_support::test_e5::e5_d8_rolling_ball_stream();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 13;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("the source fits the service input limit");
        let error = e5_rolling_ball_jets(&ctx, &bytes).expect_err("two stations need 14 slots");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "decode CATIA E5 rolling-ball stations"));
    }

    #[test]
    fn e5_rolling_ball_result_refuses_before_growth() {
        let bytes = crate::test_support::test_e5::e5_d8_rolling_ball_stream();
        assert!(matches!(
            crate::test_support::with_collection_limit(14, |ctx| e5_rolling_ball_jets(ctx, &bytes)),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "catia_e5_rolling_ball_jets"
        ));
    }

    #[test]
    fn e5_rolling_ball_definition_refuses_before_station_copy() {
        let bytes = crate::test_support::test_e5::e5_d8_rolling_ball_stream();
        let jets = decoded_jets(&bytes).expect("service resource budget");
        assert_eq!(jets.len(), 1);
        assert!(matches!(
            crate::test_support::with_collection_limit(1, |ctx| jets[0].definition(ctx)),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "catia_e5_rolling_ball_definition_stations"
        ));
    }

    fn nurbs_surface_payload(mode: u16) -> Vec<u8> {
        let mut payload = vec![0x80];
        append_nurbs_axis(&mut payload, 1);
        append_nurbs_axis(&mut payload, 1);
        payload.extend_from_slice(&mode.to_le_bytes());
        for point in [
            [0.0_f64, 0.0, 0.0],
            [0.0_f64, 1.0, 0.0],
            [1.0_f64, 0.0, 0.0],
            [1.0_f64, 1.0, 0.0],
        ] {
            for value in point {
                payload.extend_from_slice(&value.to_le_bytes());
            }
        }
        if mode == 1 {
            for weight in [1.0_f64, 1.0, 1.0, 1.0] {
                payload.extend_from_slice(&weight.to_le_bytes());
            }
        }
        payload.extend_from_slice(&[0; 148]);
        payload
    }

    #[test]
    fn e5_width_coded_reference_widens_before_shifting() {
        assert_eq!(e5_ref(&[0x10, 0xff], 0), Some((0xff00, 2)));
    }

    #[test]
    fn e7_nurbs_surface_decodes_polynomial_and_rational_modes() {
        for mode in [0, 1] {
            let mut bytes = Vec::new();
            append_e5_record(&mut bytes, 0xe7, 116, &nurbs_surface_payload(mode));
            let surfaces = crate::test_support::with_service_context(|ctx| e5_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())).expect("service resource budget");
            let [surface] = surfaces.as_slice() else {
                panic!("E7 surface did not decode");
            };
            let Some(SolvedSurfaceGeometry::Nurbs(nurbs)) = surface.geometry.solved() else {
                panic!("E7 surface was not NURBS");
            };
            assert_eq!(nurbs.u_degree(), 1);
            assert_eq!(nurbs.v_degree(), 1);
            assert_eq!(nurbs.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.u_count(), 2);
            assert_eq!(nurbs.v_count(), 2);
            assert_eq!(nurbs.poles().len(), 4);
            assert_eq!(nurbs.weights().is_some(), mode == 1);
        }
    }

    #[test]
    fn e7_nurbs_surface_requires_its_fixed_trailing_lane() {
        let mut payload = nurbs_surface_payload(0);
        payload.pop();
        let mut bytes = Vec::new();
        append_e5_record(&mut bytes, 0xe7, 116, &payload);
        assert!(crate::test_support::with_service_context(|ctx| e5_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())).expect("service resource budget").is_empty());
    }

    #[test]
    fn e7_nurbs_surface_collections_refuse_before_growth() {
        for (mode, expected) in [
            (0, [
                "catia_e5_nurbs_axis_knots",
                "catia_e5_nurbs_axis_multiplicities",
                "catia_e5_nurbs_expanded_axis",
                "catia_e5_nurbs_control_points",
                "catia_e5_nurbs_point_row",
                "catia_e5_nurbs_point_rows",
                "catia_e5_surfaces",
            ].as_slice()),
            (1, [
                "catia_e5_nurbs_weights",
                "catia_e5_nurbs_weight_row",
                "catia_e5_nurbs_weight_rows",
                "catia_e5_nurbs_weighted_poles",
                "catia_e5_nurbs_weighted_rows",
            ].as_slice()),
        ] {
            let mut bytes = Vec::new();
            append_e5_record(&mut bytes, 0xe7, 116, &nurbs_surface_payload(mode));
            let mut operations = std::collections::HashSet::new();
            for cap in 0..64 {
                if let Err(CodecError::ResourceLimit(limit)) = crate::test_support::with_collection_limit(cap, |ctx| {
                    e5_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
                }) {
                    operations.insert(limit.operation);
                }
            }
            for operation in expected {
                assert!(operations.contains(operation), "no refusal at {operation}");
            }
        }
    }

    #[test]
    fn f1_surface_wrapper_reads_its_five_reference_lane() {
        let mut payload = vec![0x85];
        for reference in [0x0102_0304, 0x0506, 0x0708, 0x090a, 0x0b0c] {
            if reference > 0xff {
                payload.push(0x18);
                payload.extend_from_slice(&(reference as u16).to_le_bytes());
            } else {
                payload.push(0x80 + reference as u8);
            }
        }
        payload.extend_from_slice(&[0; 28]);
        let mut bytes = Vec::new();
        append_e5_record(&mut bytes, 0xf1, 0x0102_0305, &payload);

        let wrappers = crate::test_support::with_service_context(|ctx| e5_surface_wrappers(ctx, &bytes)).expect("service resource budget");
        assert_eq!(wrappers.len(), 1);
        assert_eq!(wrappers[0].record_id, 0x0102_0305);
        assert_eq!(
            wrappers[0].references,
            [0x0304, 0x0506, 0x0708, 0x090a, 0x0b0c]
        );
        assert_eq!(wrappers[0].underlying_surface(), 0x0304);

        assert!(matches!(
            crate::test_support::with_collection_limit(0, |ctx| e5_surface_wrappers(ctx, &bytes)),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "catia_e5_surface_wrappers"
        ));
    }

    #[test]
    fn f1_surface_wrapper_rejects_wrong_reference_count_or_tail_size() {
        let mut bytes = Vec::new();
        let mut wrong_count = vec![0x84, 0x81, 0x82, 0x83, 0x84];
        wrong_count.extend_from_slice(&[0; 39]);
        append_e5_record(&mut bytes, 0xf1, 1, &wrong_count);
        let mut wrong_tail = vec![0x85, 0x81, 0x82, 0x83, 0x84, 0x85];
        wrong_tail.extend_from_slice(&[0; 27]);
        append_e5_record(&mut bytes, 0xf1, 2, &wrong_tail);
        assert!(crate::test_support::with_service_context(|ctx| e5_surface_wrappers(ctx, &bytes)).expect("service resource budget").is_empty());
    }
}
