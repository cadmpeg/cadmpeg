//! E5 storage-variant record decoders.
//!
//! Decodes E5 `05 08 01` vertex rosters, inline `0xc9` circle carriers,
//! class-`0xc8` planes, `0xff` edge-use records, and cylinder/cone/torus
//! analytic surface carriers.

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsSurface, ProceduralSurfaceDefinition, RollingBallJetDerivative,
    RollingBallJetSite, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;

use crate::wire::bytes::{f64_le, f64_point, f64_vector, read_f64_array, u32_le_24};
use crate::wire::records::scan_vertex_records;

/// A directly decoded E5 circle carrier.
#[derive(Debug, Clone)]
pub struct E5Circle {
    /// Offset of the `e5 0d 03` record in the source buffer.
    pub pos: usize,
    /// The complete circle carrier.
    pub geometry: CurveGeometry,
}

/// Partial class-`0xc8` plane carrier containing the fields stored directly.
#[derive(Debug, Clone)]
pub struct E5Plane {
    /// Offset of the framed record.
    pub pos: usize,
    /// Stream-assigned record identifier.
    pub record_id: u32,
    /// Stored plane origin.
    pub origin: [f64; 3],
    /// Natural U-coordinate bounds.
    #[cfg(test)]
    pub u_range: [f64; 2],
    /// Natural V-coordinate bounds.
    #[cfg(test)]
    pub v_range: [f64; 2],
}

/// A directly decoded E5 analytic surface carrier.
#[derive(Debug, Clone)]
pub struct E5Surface {
    /// Offset of the `e5 0d 03` record in the source buffer.
    pub pos: usize,
    /// Persistent E5 record id.
    pub record_id: u32,
    /// The complete analytic surface carrier.
    pub geometry: SurfaceGeometry,
    /// Component-wise scale from native E5 UV coordinates to neutral UV.
    pub uv_scale: [f64; 2],
}

/// A class-`0xd8` E5 rolling-ball surface carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct E5RollingBallJet {
    /// Offset of the framed record.
    pub pos: usize,
    /// Persistent E5 record id.
    pub record_id: u32,
    /// Degree of every scalar jet channel.
    pub degree: u32,
    /// Strictly increasing native spine parameters.
    pub knots: Vec<f64>,
    /// Multiplicities aligned with [`Self::knots`].
    pub multiplicities: Vec<u32>,
    /// Position and derivative channels at each station.
    pub sites: Vec<RollingBallJetSite>,
    /// Native parameter interval repeated in the carrier tail.
    pub parameter_range: [f64; 2],
    /// Native radius repeated in the carrier tail.
    pub radius: f64,
    /// Native surface-sense flag retained without reinterpretation.
    pub sense: i32,
}

impl E5RollingBallJet {
    /// Convert the admitted carrier payload to the exact neutral jet form.
    #[must_use]
    pub fn definition(&self) -> ProceduralSurfaceDefinition {
        ProceduralSurfaceDefinition::RollingBallJet {
            degree: self.degree,
            knots: self.knots.clone(),
            multiplicities: self.multiplicities.clone(),
            sites: self.sites.clone(),
        }
    }
}

/// A class-`0xf1` surface wrapper.
///
/// The wrapper keeps the five serialized references. Its first reference is
/// the underlying surface carrier when the wrapper is used by an E5 face;
/// the remaining references and tail are retained structurally by the frame
/// but are not assigned a separate meaning here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E5SurfaceWrapper {
    /// Offset of the framed record.
    pub pos: usize,
    /// Persistent E5 record id.
    pub record_id: u32,
    /// Five references in serialized order.
    pub references: [u32; 5],
}

impl E5SurfaceWrapper {
    /// The first reference, which names the wrapped geometric carrier.
    #[must_use]
    pub fn underlying_surface(&self) -> u32 {
        self.references[0]
    }
}

#[derive(Clone, Copy)]
struct E5Record {
    pos: usize,
    end: usize,
    class: u8,
    size: usize,
}

const MARKER: &[u8; 3] = &crate::layout::token::E5_RECORD_FAMILY;
const E5_NURBS_SURFACE_TAIL_BYTES: usize = 148;
const E5_D8_TAIL_BYTES: usize = 63;
const E5_D8_ARC_TOLERANCE: f64 = 1e-8;
const E5_D8_RADIUS_TOLERANCE: f64 = 1e-8;

fn e5_records(data: &[u8]) -> Vec<E5Record> {
    debug_assert_eq!(MARKER, crate::container::E5_MARKER);
    crate::container::all_e5_record_spans(data)
        .into_iter()
        .filter_map(|range| {
            let pos = range.start;
            let size = View::u16_le_at(data, pos + 5).map(usize::from)?;
            Some(E5Record {
                pos,
                end: range.end,
                class: data[pos + 3],
                size,
            })
        })
        .collect()
}

/// Read the complete ordered E5 `05 08 01` coordinate roster matching the
/// referenced vertex population. The roster may be split into multiple runs;
/// marker-like bytes inside framed payloads are not vertex rows.
#[must_use]
pub fn e5_vertices(data: &[u8], vertex_count: usize) -> Vec<Point3> {
    if vertex_count == 0 {
        return Vec::new();
    }
    let records = e5_records(data);
    let mut runs = Vec::new();
    let mut region_start = 0usize;
    for record in records {
        runs.extend(vertex_runs(&data[region_start..record.pos]));
        region_start = record.end;
    }
    runs.extend(vertex_runs(&data[region_start..]));
    let Some(run_count) = runs
        .iter()
        .try_fold(0usize, |count, run| count.checked_add(run.len()))
    else {
        return Vec::new();
    };
    if run_count != vertex_count {
        return Vec::new();
    }
    runs.into_iter().flatten().collect()
}

fn vertex_runs(bytes: &[u8]) -> Vec<Vec<Point3>> {
    let mut runs = Vec::new();
    let mut position = 0usize;
    while position + 15 <= bytes.len() {
        if bytes[position..position + 3] != [0x05, 0x08, 0x01] {
            position += 1;
            continue;
        }
        let start = position;
        while position + 15 <= bytes.len() && bytes[position..position + 3] == [0x05, 0x08, 0x01] {
            position += 15;
        }
        let vertices = scan_vertex_records(&bytes[start..position]);
        if !vertices.is_empty() {
            runs.push(vertices);
        }
    }
    runs
}

/// Walk an E5 record stream and decode its inline `0xc9` circle carriers.
/// Record strides are derived from the little-endian size field at `+5`.
pub fn e5_circles(data: &[u8]) -> Vec<E5Circle> {
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
                if radius.is_finite() && radius > 0.0 {
                    if let Some(axis) = frame_u.cross(frame_v).unit() {
                        out.push(E5Circle {
                            pos,
                            geometry: CurveGeometry::Circle {
                                center: origin,
                                axis,
                                ref_direction: frame_u.unit().unwrap_or_else(|| {
                                    cadmpeg_ir::geometry::derive_reference_direction(axis)
                                }),
                                radius,
                            },
                        });
                    }
                }
            }
        }
    }
    out
}

/// Decode the byte-explicit origin and natural bounds of E5 class-`0xc8` planes.
///
/// The record does not store a complete in-plane frame, so this function does not
/// synthesize plane axes or a [`SurfaceGeometry`].
#[must_use]
pub fn e5_planes(data: &[u8]) -> Vec<E5Plane> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        if record.class != 0xc8 || record.size < 90 || (record.size - 90) % 8 != 0 {
            continue;
        }
        let Some(origin) = read_f64_array::<3>(data, pos + 14) else {
            continue;
        };
        let scalar_count = (record.size - 58) / 8;
        let scalars_finite = (0..scalar_count)
            .all(|index| f64_le(data, pos + 39 + 8 * index).is_some_and(f64::is_finite));
        let Some(bounds) = read_f64_array::<4>(data, record.end - 32) else {
            continue;
        };
        if !scalars_finite || origin.iter().chain(&bounds).any(|value| !value.is_finite()) {
            continue;
        }
        out.push(E5Plane {
            pos,
            record_id: View::u32_le_at(data, pos + 9).unwrap_or(0),
            origin,
            #[cfg(test)]
            u_range: [bounds[0], bounds[1]],
            #[cfg(test)]
            v_range: [bounds[2], bounds[3]],
        });
    }
    out
}

/// A directly framed E5 edge-use record.  The endpoint ids are E5 vertex
/// records, not point-table indexes.
#[derive(Debug, Clone)]
pub struct E5Edge {
    /// Referenced start-vertex (class `0xfe`) record id.
    pub start_vertex_id: u32,
    /// Referenced end-vertex (class `0xfe`) record id.
    pub end_vertex_id: u32,
}

/// Decode E5 `0xff` five-reference edge records.
pub fn e5_edges(data: &[u8]) -> Vec<E5Edge> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        if record.class == 0xff && data.get(pos + 13) == Some(&0x85) {
            let payload = &data[pos + 13..record.end];
            if let Some((_, next)) = e5_ref(payload, 1) {
                if let Some((start_vertex_id, next)) = e5_ref(payload, next) {
                    if let Some((end_vertex_id, _)) = e5_ref(payload, next) {
                        out.push(E5Edge {
                            start_vertex_id,
                            end_vertex_id,
                        });
                    }
                }
            }
        }
    }
    out
}

/// Decode E5 cylinder (`0xc9`), cone (`0xca`), and torus (`0xcc`) surface
/// records. The E5 plane class does not serialize a standalone normal.
pub fn e5_surfaces(data: &[u8]) -> Vec<E5Surface> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        let pos = record.pos;
        let decoded = match record.class {
            0xc9 => e5_cylinder(data, pos).and_then(|geometry| {
                let SurfaceGeometry::Cylinder { radius, .. } = geometry else {
                    unreachable!()
                };
                let parameter_scale = [1.0 / radius, 1.0];
                parameter_scale
                    .into_iter()
                    .all(f64::is_finite)
                    .then_some((geometry, parameter_scale))
            }),
            0xca => e5_cone(data, pos).and_then(|geometry| {
                let SurfaceGeometry::Cone { half_angle, .. } = geometry else {
                    unreachable!()
                };
                let u_scale = f64_le(data, pos + 158)?;
                let v_scale = f64_le(data, pos + 166)?;
                let parameter_scale = [1.0 / u_scale, half_angle.cos() / v_scale];
                (u_scale.is_finite()
                    && u_scale != 0.0
                    && v_scale.is_finite()
                    && v_scale != 0.0
                    && parameter_scale.into_iter().all(f64::is_finite))
                .then_some((geometry, parameter_scale))
            }),
            0xcc => e5_torus(data, pos).and_then(|geometry| {
                let SurfaceGeometry::Torus {
                    major_radius,
                    minor_radius,
                    ..
                } = geometry
                else {
                    unreachable!()
                };
                let parameter_scale = [1.0 / major_radius, 1.0 / minor_radius];
                parameter_scale
                    .into_iter()
                    .all(f64::is_finite)
                    .then_some((geometry, parameter_scale))
            }),
            0xe7 => e5_nurbs_surface(data, record).map(|geometry| (geometry, [1.0, 1.0])),
            _ => None,
        };
        if let Some((geometry, uv_scale)) = decoded {
            out.push(E5Surface {
                pos,
                record_id: View::u32_le_at(data, pos + 9).unwrap_or(0),
                geometry,
                uv_scale,
            });
        }
    }
    out
}

/// Decode E5 class-`0xd8` rolling-ball surface carriers.
///
/// The carrier stores three structure-of-arrays lanes of ten f64 channels:
/// position, first derivative, and second derivative. The first six channels
/// are the two limiting points, the next three are the centre, and the last
/// channel is the opening angle. The tail range, radius, and sense are kept on
/// the native record; the neutral definition contains the complete value and
/// derivative jets.
#[must_use]
pub fn e5_rolling_ball_jets(data: &[u8]) -> Vec<E5RollingBallJet> {
    e5_records(data)
        .into_iter()
        .filter(|record| record.class == 0xd8)
        .filter_map(|record| parse_e5_rolling_ball_jet(data, record))
        .collect()
}

fn parse_e5_rolling_ball_jet(data: &[u8], record: E5Record) -> Option<E5RollingBallJet> {
    let expected_end = record.pos.checked_add(13)?.checked_add(record.size)?;
    if expected_end != record.end {
        return None;
    }
    let mut view = View::over_retained(data).child(record.pos.checked_add(13)?, record.end)?;
    if view.u8()? != 0x80 {
        return None;
    }
    let station_count = usize::try_from(view.u32_le()?).ok()?;
    let degree = view.u32_le()?;
    let zero0 = view.u32_le()?;
    let zero1 = view.u32_le()?;
    let repeated_station_count = usize::try_from(view.u32_le()?).ok()?;
    let zero2 = view.u32_le()?;
    if station_count < 2
        || degree != 5
        || repeated_station_count != station_count
        || [zero0, zero1, zero2] != [0; 3]
        || record.size
            != station_count
                .checked_mul(252)
                .and_then(|size| size.checked_add(88))?
    {
        return None;
    }
    let station_count_u64 = u64::try_from(station_count).ok()?;
    let knots = view.read_counted(station_count_u64, 8, View::f64_le)?;
    if knots.iter().any(|knot| !knot.is_finite()) || knots.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return None;
    }
    let multiplicities = view.read_counted(station_count_u64, 4, View::u32_le)?;
    if multiplicities.first() != Some(&6)
        || multiplicities.last() != Some(&6)
        || multiplicities
            .iter()
            .skip(1)
            .take(station_count.saturating_sub(2))
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
    let sense = view.i32_le()?;
    let tail_zero1 = view.f64_le()?;
    let tail_radius2 = view.f64_le()?;
    if parameter_min.to_bits() != knots.first()?.to_bits()
        || parameter_max.to_bits() != knots.last()?.to_bits()
        || tail_zero0.to_bits() != 0
        || tail_zero1.to_bits() != 0
        || !parameter_min.is_finite()
        || !parameter_max.is_finite()
        || !tail_radius0.is_finite()
        || tail_radius0 <= 0.0
        || !relative_close(tail_radius0, tail_radius1, E5_D8_RADIUS_TOLERANCE)
        || !relative_close(tail_radius0, tail_radius2, E5_D8_RADIUS_TOLERANCE)
        || !matches!(sense, -1 | 1)
        || view.array::<3>()? != [1, 0, 0]
    {
        return None;
    }
    let sites = positions
        .into_iter()
        .zip(first_derivatives)
        .zip(second_derivatives)
        .map(|((position, first), second)| {
            let first_limit = Point3::new(position[0], position[1], position[2]);
            let second_limit = Point3::new(position[3], position[4], position[5]);
            let center = Point3::new(position[6], position[7], position[8]);
            let radius = d8_distance(center, first_limit);
            let second_radius = d8_distance(center, second_limit);
            let expected_angle = if radius > 0.0 && second_radius > 0.0 {
                first_limit
                    .vector_from(center)
                    .scale(1.0 / radius)
                    .dot(second_limit.vector_from(center).scale(1.0 / second_radius))
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
            first_limit,
            second_limit,
            center,
            radius,
            second_radius,
            expected_angle,
            stored_angle,
            first,
            second,
        )| {
            ![first_limit.x, first_limit.y, first_limit.z]
                .into_iter()
                .chain([second_limit.x, second_limit.y, second_limit.z])
                .chain([center.x, center.y, center.z])
                .all(f64::is_finite)
                || !radius.is_finite()
                || *radius <= 0.0
                || !second_radius.is_finite()
                || !relative_close(*radius, *second_radius, E5_D8_RADIUS_TOLERANCE)
                || !stored_angle.is_finite()
                || !expected_angle.is_finite()
                || (stored_angle - expected_angle).abs() > E5_D8_ARC_TOLERANCE
                || first.iter().chain(second).any(|value| !value.is_finite())
                || !relative_close(*radius, tail_radius0, E5_D8_RADIUS_TOLERANCE)
        },
    ) {
        return None;
    }
    let sites = sites
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
                first_derivative: d8_derivative(first),
                second_derivative: d8_derivative(second),
            },
        )
        .collect();
    Some(E5RollingBallJet {
        pos: record.pos,
        record_id: View::u32_le_at(data, record.pos + 9)?,
        degree,
        knots,
        multiplicities,
        sites,
        parameter_range: [parameter_min, parameter_max],
        radius: tail_radius0,
        sense,
    })
}

fn read_d8_channel_rows(view: &mut View<'_>, station_count_u64: u64) -> Option<Vec<[f64; 10]>> {
    view.read_counted(station_count_u64, 80, |view| {
        let mut row = [0.0; 10];
        for value in &mut row {
            *value = view.f64_le()?;
        }
        Some(row)
    })
}

fn d8_derivative(values: [f64; 10]) -> RollingBallJetDerivative {
    RollingBallJetDerivative {
        first_limit: Vector3::new(values[0], values[1], values[2]),
        second_limit: Vector3::new(values[3], values[4], values[5]),
        center: Vector3::new(values[6], values[7], values[8]),
        angle: values[9],
    }
}

fn d8_distance(left: Point3, right: Point3) -> f64 {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
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
pub fn e5_surface_wrappers(data: &[u8]) -> Vec<E5SurfaceWrapper> {
    let mut out = Vec::new();
    for record in e5_records(data) {
        if record.class != 0xf1 || record.size != 44 {
            continue;
        }
        let Some((references, next)) =
            crate::wire::counted_refs(&data[record.pos + 13..record.end], false)
        else {
            continue;
        };
        let Ok(references) = <[u32; 5]>::try_from(references) else {
            continue;
        };
        if next >= 44 {
            continue;
        }
        out.push(E5SurfaceWrapper {
            pos: record.pos,
            record_id: View::u32_le_at(data, record.pos + 9).unwrap_or(0),
            references,
        });
    }
    out
}

fn e5_nurbs_surface(data: &[u8], record: E5Record) -> Option<SurfaceGeometry> {
    let mut view = View::over_retained(data).child(record.pos + 13, record.end)?;
    if view.u8()? != 0x80 {
        return None;
    }
    let (u_degree, u_knots, u_multiplicities) = read_nurbs_axis(&mut view)?;
    let (v_degree, v_knots, v_multiplicities) = read_nurbs_axis(&mut view)?;
    let (u_knots, u_count) = expand_nurbs_axis(u_degree, &u_knots, &u_multiplicities, record.size)?;
    let (v_knots, v_count) = expand_nurbs_axis(v_degree, &v_knots, &v_multiplicities, record.size)?;
    let mode = view.u16_le()?;
    if !matches!(mode, 0 | 1) {
        return None;
    }
    let control_count = u_count.checked_mul(v_count)?;
    let control_points = view.read_counted(u64::try_from(control_count).ok()?, 24, |view| {
        Some(Point3::new(view.f64_le()?, view.f64_le()?, view.f64_le()?))
    })?;
    let weights = if mode == 1 {
        Some(view.read_counted(u64::try_from(control_count).ok()?, 8, View::f64_le)?)
    } else {
        None
    };
    if control_points
        .iter()
        .any(|point| ![point.x, point.y, point.z].into_iter().all(f64::is_finite))
        || weights.as_ref().is_some_and(|weights| {
            weights
                .iter()
                .copied()
                .any(|weight| !weight.is_finite() || weight == 0.0)
        })
        || view.remaining() != E5_NURBS_SURFACE_TAIL_BYTES
    {
        return None;
    }
    view.skip(E5_NURBS_SURFACE_TAIL_BYTES)?;
    view.is_empty()
        .then(|| {
            NurbsSurface::new(
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                u32::try_from(u_count).ok()?,
                u32::try_from(v_count).ok()?,
                control_points,
                weights,
                false,
                false,
                false,
            )
            .ok()
            .map(SurfaceGeometry::Nurbs)
        })
        .flatten()
}

fn read_nurbs_axis(view: &mut View<'_>) -> Option<(u32, Vec<f64>, Vec<u32>)> {
    let degree = view.u32_le()?;
    let zero0 = view.u32_le()?;
    let zero1 = view.u32_le()?;
    let knot_count = usize::try_from(view.u32_le()?).ok()?;
    let zero2 = view.u32_le()?;
    if degree == 0 || [zero0, zero1, zero2] != [0; 3] || knot_count == 0 {
        return None;
    }
    let knot_count_u64 = u64::try_from(knot_count).ok()?;
    let knots = view.read_counted(knot_count_u64, 8, View::f64_le)?;
    let multiplicities = view.read_counted(knot_count_u64, 4, View::u32_le)?;
    Some((degree, knots, multiplicities))
}

fn expand_nurbs_axis(
    degree: u32,
    knots: &[f64],
    multiplicities: &[u32],
    payload_size: usize,
) -> Option<(Vec<f64>, usize)> {
    if knots.len() != multiplicities.len()
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities.contains(&0)
    {
        return None;
    }
    let total = multiplicities
        .iter()
        .try_fold(0usize, |total, multiplicity| {
            total.checked_add(usize::try_from(*multiplicity).ok()?)
        })?;
    if total > payload_size {
        return None;
    }
    let degree = usize::try_from(degree).ok()?;
    let control_count = total.checked_sub(degree.checked_add(1)?)?;
    if control_count <= degree || knots.first()? >= knots.last()? {
        return None;
    }
    let mut expanded = Vec::with_capacity(total);
    for (knot, multiplicity) in knots.iter().zip(multiplicities) {
        expanded.extend(std::iter::repeat_n(
            *knot,
            usize::try_from(*multiplicity).ok()?,
        ));
    }
    (expanded.len() == total).then_some((expanded, control_count))
}

fn e5_cylinder(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14);
    let origin = c.point3()?;
    let (geometry, radius) = crate::analytic::cylinder_uvr(&mut c, origin)?;
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    Some(geometry)
}

fn e5_cone(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14);
    let (geometry, radius, half_angle) = crate::analytic::cone_ozra(&mut c)?;
    if !(radius.is_finite()
        && radius > 0.0
        && half_angle.is_finite()
        && half_angle > 0.0
        && half_angle < std::f64::consts::FRAC_PI_2)
    {
        return None;
    }
    Some(geometry)
}

fn e5_torus(data: &[u8], pos: usize) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(data, pos + 14);
    let (geometry, major_radius, minor_radius) = crate::analytic::torus_ozrr(&mut c)?;
    if !(major_radius.is_finite()
        && major_radius > 0.0
        && minor_radius.is_finite()
        && minor_radius > 0.0)
    {
        return None;
    }
    Some(geometry)
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
    use cadmpeg_ir::math::{Point3, Vector3};

    use super::{e5_ref, e5_rolling_ball_jets, e5_surface_wrappers, e5_surfaces};

    const TEST_F64_TOLERANCE: f64 = 1e-12;

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

    fn append_e5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
        bytes.extend_from_slice(&[0xe5, 0x0d, 0x03, class, 0]);
        bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(payload);
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
        let bytes = crate::test_support::e5_d8_rolling_ball_stream();

        let jets = e5_rolling_ball_jets(&bytes);
        assert_eq!(jets.len(), 1);
        let jet = &jets[0];
        assert_eq!(jet.record_id, 42);
        assert_eq!(jet.degree, 5);
        assert_eq!(jet.knots.len(), 2);
        assert_close(jet.knots[0], 2.0);
        assert_close(jet.knots[1], 5.0);
        assert_eq!(jet.multiplicities, [6, 6]);
        assert_close(jet.parameter_range[0], 2.0);
        assert_close(jet.parameter_range[1], 5.0);
        assert_close(jet.radius, 2.0);
        assert_eq!(jet.sense, -1);
        assert_point_close(jet.sites[0].first_limit, Point3::new(2.0, 0.0, 0.0));
        assert_point_close(jet.sites[1].center, Point3::new(1.0, 0.0, 0.0));
        assert_close(jet.sites[0].angle, std::f64::consts::FRAC_PI_2);
        assert_vector_close(
            jet.sites[0].first_derivative.center,
            Vector3::new(0.7, 0.8, 0.9),
        );
        assert_vector_close(
            jet.sites[0].second_derivative.center,
            Vector3::new(2.7, 2.8, 2.9),
        );
        assert_close(jet.sites[1].second_derivative.angle, 4.0);
        assert!(matches!(
            jet.definition(),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
                degree: 5,
                ref knots,
                ref multiplicities,
                ref sites,
            } if knots.len() == 2 && multiplicities == &[6, 6] && sites.len() == 2
        ));
    }

    #[test]
    fn d8_record_rejects_wrong_tail_marker() {
        let mut payload = crate::test_support::e5_d8_rolling_ball_stream();
        let last = payload.len() - 3;
        payload[last] = 0;
        assert!(e5_rolling_ball_jets(&payload).is_empty());
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
            let surfaces = e5_surfaces(&bytes);
            let [surface] = surfaces.as_slice() else {
                panic!("E7 surface did not decode");
            };
            let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &surface.geometry else {
                panic!("E7 surface was not NURBS");
            };
            assert_eq!(nurbs.u_degree(), 1);
            assert_eq!(nurbs.v_degree(), 1);
            assert_eq!(nurbs.u_knots(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.v_knots(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.u_count(), 2);
            assert_eq!(nurbs.v_count(), 2);
            assert_eq!(nurbs.control_points().len(), 4);
            assert_eq!(nurbs.weights().is_some(), mode == 1);
        }
    }

    #[test]
    fn e7_nurbs_surface_requires_its_fixed_trailing_lane() {
        let mut payload = nurbs_surface_payload(0);
        payload.pop();
        let mut bytes = Vec::new();
        append_e5_record(&mut bytes, 0xe7, 116, &payload);
        assert!(e5_surfaces(&bytes).is_empty());
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

        let wrappers = e5_surface_wrappers(&bytes);
        assert_eq!(wrappers.len(), 1);
        assert_eq!(wrappers[0].record_id, 0x0102_0305);
        assert_eq!(
            wrappers[0].references,
            [0x0304, 0x0506, 0x0708, 0x090a, 0x0b0c]
        );
        assert_eq!(wrappers[0].underlying_surface(), 0x0304);
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
        assert!(e5_surface_wrappers(&bytes).is_empty());
    }
}
