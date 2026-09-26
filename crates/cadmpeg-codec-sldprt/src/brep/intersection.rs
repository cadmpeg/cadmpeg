// SPDX-License-Identifier: Apache-2.0
//! Surface-intersection curve carriers.
//!
//! A `00 26` composite record or `00 01 5a` intersection-data entity carries a
//! curve defined by the intersection of two support surfaces. Its payload
//! references a `00 28` chart record (the solved point cache), two `00 29`
//! terminator records (the exact curve endpoints), and a `00 cc` support-UV
//! record. Referenced terminators select the chart entry width and replace its
//! approximate endpoints. A complete width-4 UV record additionally yields
//! co-parameterized support pcurve caches.

use std::collections::HashMap;

use cadmpeg_core::bytes::find_iter;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::loss::LossNote;

use super::{CurveCarrier, LEN_TO_MM};

use crate::layout::intersection_composite as isect;
use crate::layout::support_uv_00_cc as support_uv;

/// Chart parameter sentinel marking an absent value.
const MISSING_PARAMETER: f64 = -31_415_800_000_000.0;
/// Fixed bytes between an inline `term_use` label and its terminator body.
const INLINE_TERM_TAIL: &[u8] = b"\x00\x00\x00\x01\x01\x63\x43\x5a";
/// Fixed bytes between an inline `values` label and its support-UV body.
const INLINE_UV_TAIL: &[u8] = b"\x00\x00\x00\x02\x01\x66\x01";

/// One decoded chart: solved points in metres and parameter bookkeeping.
struct Chart {
    endpoints: [[f64; 3]; 2],
    interior_points: Vec<[f64; 3]>,
    base_parameter: f64,
    base_scale: f64,
    chordal_error: f64,
}

/// One validated intersection curve and its solved chart.
pub(super) struct IntersectionCarrier {
    pub(super) carrier: CurveCarrier,
    pub(super) support_data: IntersectionSupportData,
}

/// Ordered supports and optional UV lanes for the model-space chart curve.
#[derive(Clone)]
pub(super) struct IntersectionSupportData {
    pub(super) supports: [u16; 2],
    pub(super) fit_tolerance_mm: f64,
    pub(super) support_uv: Option<[Vec<Point2>; 2]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UvWidth {
    Two,
    Four,
}

impl UvWidth {
    fn values_per_point(self) -> usize {
        match self {
            Self::Two => 2,
            Self::Four => 4,
        }
    }
}

struct UvRecord {
    width: UvWidth,
    values: Vec<f64>,
}

struct SolvedChart {
    geometry: CurveGeometry,
    parameters: Vec<f64>,
    fit_tolerance_mm: f64,
    reversed: bool,
    endpoint_displacement: f64,
}

/// Offsets of every `00 tt` tag, with the optional `0xff` escape skipped.
fn record_bodies(bytes: &[u8], tt: u8) -> impl Iterator<Item = usize> + '_ {
    (0..bytes.len()).filter_map(move |at| {
        if bytes.get(at) != Some(&0x00) || bytes.get(at + 1) != Some(&tt) {
            return None;
        }
        let body = at + 2;
        Some(if bytes.get(body) == Some(&0xff) {
            body + 1
        } else {
            body
        })
    })
}

/// Carrier, body, and payload-marker offsets for both intersection forms.
fn composite_records(bytes: &[u8]) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
    record_bodies(bytes, 0x26)
        .filter_map(|body| {
            let marker = body.checked_add(isect::MARKER)?;
            matches!(bytes.get(marker), Some(0x2b | 0x2d)).then_some((body - 2, body, marker))
        })
        .chain(
            (0..bytes.len().checked_sub(20).map_or(0, |end| end)).filter_map(|offset| {
                if bytes.get(offset..offset + 3) != Some(&[0x00, 0x01, 0x5a]) {
                    return None;
                }
                let body = offset + 3;
                let marker = body + isect::MARKER;
                matches!(bytes.get(marker), Some(0x2b | 0x2d)).then_some((offset, body, marker))
            }),
        )
}

fn finite_point(bytes: &[u8], at: usize) -> Option<[f64; 3]> {
    let point = [
        View::f64_be_at(bytes, at)?,
        View::f64_be_at(bytes, at + 8)?,
        View::f64_be_at(bytes, at + 16)?,
    ];
    point
        .iter()
        .all(|value| value.is_finite() && value.abs() < 1e6)
        .then_some(point)
}

fn finite_tangent(bytes: &[u8], at: usize) -> bool {
    let Some(tangent) = (|| {
        Some([
            View::f64_be_at(bytes, at)?,
            View::f64_be_at(bytes, at + 8)?,
            View::f64_be_at(bytes, at + 16)?,
        ])
    })() else {
        return false;
    };
    if tangent.iter().any(|value| !value.is_finite()) {
        return false;
    }
    tangent.iter().any(|value| *value != 0.0)
}

/// Parse every `00 28` chart record: `count:u32 attr:u16 base_parameter:f64
/// base_scale:f64 chart_count:u32 chordal_error:f64`, two [`MISSING_PARAMETER`]
/// sentinels at +36/+44, then `count` point entries at +52 (88-byte entries
/// carrying a finite nonzero tangent at +56, or bare 24-byte points).
fn chart_records(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
) -> Result<HashMap<u16, Vec<Chart>>, CodecError> {
    let mut out: HashMap<u16, Vec<Chart>> = HashMap::new();
    for body in record_bodies(bytes, 0x28) {
        let Some((attr, candidates)) = chart_candidates(ctx, bytes, body)? else {
            continue;
        };
        if let Some(ctx) = ctx {
            if !out.contains_key(&attr) {
                ctx.charge_collection_items(1, "collect Parasolid intersection charts")?;
            }
            ctx.charge_collection_items(
                candidates.len() as u64,
                "collect Parasolid intersection chart candidates",
            )?;
        }
        out.entry(attr).or_default().extend(candidates);
    }
    Ok(out)
}

fn chart_candidates(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    body: usize,
) -> Result<Option<(u16, Vec<Chart>)>, CodecError> {
    let Some(count) = View::u32_be_at(bytes, body).and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    let Some(attr) = View::u16_be_at(bytes, body + 4) else {
        return Ok(None);
    };
    let preamble = body + 6;
    let (Some(base_parameter), Some(base_scale), Some(chart_count), Some(chordal_error)) = (
        View::f64_be_at(bytes, preamble),
        View::f64_be_at(bytes, preamble + 8),
        View::u32_be_at(bytes, preamble + 16).and_then(|count| usize::try_from(count).ok()),
        View::f64_be_at(bytes, preamble + 20),
    ) else {
        return Ok(None);
    };
    if !(2..=4096).contains(&count)
        || chart_count != count
        || !base_parameter.is_finite()
        || !base_scale.is_finite()
        || base_scale == 0.0
        || !chordal_error.is_finite()
        || chordal_error <= 0.0
        || View::f64_be_at(bytes, preamble + 36) != Some(MISSING_PARAMETER)
        || View::f64_be_at(bytes, preamble + 44) != Some(MISSING_PARAMETER)
    {
        return Ok(None);
    }
    let block = preamble + 52;
    let mut candidates = Vec::new();
    for (stride, extended) in [(88usize, true), (24usize, false)] {
        let Some(end) = stride
            .checked_mul(count)
            .and_then(|size| block.checked_add(size))
        else {
            continue;
        };
        if end > bytes.len() {
            continue;
        }
        if extended && !(0..count).all(|index| finite_tangent(bytes, block + index * stride + 56)) {
            continue;
        }
        let (Some(first), Some(last)) = (
            finite_point(bytes, block),
            finite_point(bytes, block + (count - 1) * stride),
        ) else {
            continue;
        };
        if !(1..count - 1).all(|index| finite_point(bytes, block + index * stride).is_some()) {
            continue;
        }
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(
                (count - 2) as u64,
                "decode Parasolid chart interior points",
            )?;
        }
        let Some(interior_points) = (1..count - 1)
            .map(|index| finite_point(bytes, block + index * stride))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !extended && first == last && interior_points.iter().all(|point| *point == first) {
            continue;
        }
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "collect Parasolid chart stride candidates")?;
        }
        candidates.push(Chart {
            endpoints: [first, last],
            interior_points,
            base_parameter,
            base_scale,
            chordal_error,
        });
    }
    Ok((!candidates.is_empty()).then_some((attr, candidates)))
}

/// Parse a terminator body: `count:u32 attr:u16`, a kind label, then the
/// endpoint. The label is one kind character (`L` limit, `H` ring, `T`
/// terminator) with an optional second character (`?`, `F`, or `S`). Both
/// label widths yield a candidate endpoint; composite validation selects the
/// candidate that matches the chart.
fn term_at(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    body: usize,
    out: &mut HashMap<u16, Vec<[f64; 3]>>,
) -> Result<(), CodecError> {
    let (Some(count), Some(attr)) = (
        View::u32_be_at(bytes, body),
        View::u16_be_at(bytes, body + 4),
    ) else {
        return Ok(());
    };
    if !(1..=2).contains(&count) {
        return Ok(());
    }
    if !matches!(bytes.get(body + 6), Some(b'L' | b'H' | b'T')) {
        return Ok(());
    }
    let two_char = matches!(bytes.get(body + 7), Some(b'?' | b'F' | b'S'));
    for label_len in [2usize, 1] {
        if label_len == 2 && !two_char {
            continue;
        }
        if let Some(point) = finite_point(bytes, body + 6 + label_len) {
            if let Some(ctx) = ctx {
                if !out.contains_key(&attr) {
                    ctx.charge_collection_items(1, "collect Parasolid terminator groups")?;
                }
                ctx.charge_collection_items(1, "collect Parasolid terminator points")?;
            }
            out.entry(attr).or_default().push(point);
        }
    }
    Ok(())
}

/// Every `00 29` or inline `term_use` terminator, keyed by attribute.
fn term_records(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
) -> Result<HashMap<u16, Vec<[f64; 3]>>, CodecError> {
    let mut out: HashMap<u16, Vec<[f64; 3]>> = HashMap::new();
    for body in record_bodies(bytes, 0x29) {
        term_at(ctx, bytes, body, &mut out)?;
    }
    for label in find_iter(bytes, b"term_use") {
        let tail = label + b"term_use".len();
        if bytes.get(tail..tail + INLINE_TERM_TAIL.len()) == Some(INLINE_TERM_TAIL) {
            term_at(ctx, bytes, tail + INLINE_TERM_TAIL.len(), &mut out)?;
        }
    }
    Ok(out)
}

/// Parse a support-UV body: `count:u32 attr:u16 width_marker:u8(2|3|4)` then
/// `count` finite f64 values.
fn uv_at(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    body: usize,
) -> Result<Option<(u16, UvRecord)>, CodecError> {
    let (Some(count), Some(attr), Some(marker)) = (
        View::u32_be_at(bytes, body + support_uv::COUNT)
            .and_then(|count| usize::try_from(count).ok()),
        View::u16_be_at(bytes, body + support_uv::ATTR),
        bytes.get(body + support_uv::WIDTH).copied(),
    ) else {
        return Ok(None);
    };
    let width = match marker {
        2 | 3 => UvWidth::Two,
        4 => UvWidth::Four,
        _ => return Ok(None),
    };
    if count < width.values_per_point() * 2 || !count.is_multiple_of(width.values_per_point()) {
        return Ok(None);
    }
    let Some(values_at) = body.checked_add(support_uv::LEN) else {
        return Ok(None);
    };
    if cadmpeg_core::decode::bounded_len(
        count as u64,
        8,
        bytes.len().checked_sub(values_at).map_or(0, |len| len),
    ) != Some(count)
    {
        return Ok(None);
    }
    if !(0..count)
        .all(|index| View::f64_be_at(bytes, values_at + index * 8).is_some_and(f64::is_finite))
    {
        return Ok(None);
    }
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(count as u64, "decode Parasolid support UV values")?;
    }
    let values = (0..count)
        .map(|index| View::f64_be_at(bytes, body + support_uv::LEN + index * 8))
        .collect::<Option<Vec<_>>>();
    Ok(values.and_then(|values| {
        values
            .iter()
            .all(|value| value.is_finite())
            .then_some((attr, UvRecord { width, values }))
    }))
}

/// Every `00 cc` or inline `values` support-UV record, keyed by attribute.
fn uv_records(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
) -> Result<HashMap<u16, Vec<UvRecord>>, CodecError> {
    let mut out: HashMap<u16, Vec<UvRecord>> = HashMap::new();
    for body in record_bodies(bytes, 0xcc) {
        if let Some((attr, shape)) = uv_at(ctx, bytes, body)? {
            if let Some(ctx) = ctx {
                if !out.contains_key(&attr) {
                    ctx.charge_collection_items(1, "collect Parasolid support UV groups")?;
                }
                ctx.charge_collection_items(1, "collect Parasolid support UV records")?;
            }
            out.entry(attr).or_default().push(shape);
        }
    }
    for label in find_iter(bytes, b"values") {
        let tail = label + b"values".len();
        if bytes.get(tail..tail + INLINE_UV_TAIL.len()) == Some(INLINE_UV_TAIL) {
            if let Some((attr, shape)) = uv_at(ctx, bytes, tail + INLINE_UV_TAIL.len())? {
                if let Some(ctx) = ctx {
                    if !out.contains_key(&attr) {
                        ctx.charge_collection_items(1, "collect Parasolid support UV groups")?;
                    }
                    ctx.charge_collection_items(1, "collect Parasolid support UV records")?;
                }
                out.entry(attr).or_default().push(shape);
            }
        }
    }
    Ok(out)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    Point3::from(a).distance(Point3::from(b))
}

fn charge_items(
    ctx: Option<&DecodeContext<'_>>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(count as u64, operation)?;
    }
    Ok(())
}

/// Build the derived polyline curve for one validated composite.
fn solved_curve(
    ctx: Option<&DecodeContext<'_>>,
    chart: &Chart,
    start: [f64; 3],
    end: [f64; 3],
    record_attr: u16,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<(CurveGeometry, Vec<f64>, bool)>, CodecError> {
    let mut parameter = chart.base_parameter;
    let point_count = chart.interior_points.len() + 2;
    charge_items(ctx, point_count, "construct intersection chart parameters")?;
    let mut parameters = Vec::with_capacity(point_count);
    parameters.push(parameter);
    let mut previous = chart.endpoints[0];
    for &point in chart
        .interior_points
        .iter()
        .chain(std::iter::once(&chart.endpoints[1]))
    {
        parameter += distance(previous, point) * chart.base_scale;
        parameters.push(parameter);
        previous = point;
    }
    charge_items(ctx, point_count, "construct intersection chart points")?;
    let mut points = std::iter::once(start)
        .chain(chart.interior_points.iter().copied())
        .chain(std::iter::once(end))
        .collect::<Vec<_>>();
    let reversed = if parameters.windows(2).all(|pair| pair[0] < pair[1]) {
        false
    } else if parameters.windows(2).all(|pair| pair[0] > pair[1]) {
        parameters.reverse();
        points.reverse();
        true
    } else {
        return Ok(None);
    };
    let (first, last) = if reversed {
        (parameter, chart.base_parameter)
    } else {
        (chart.base_parameter, parameter)
    };
    charge_items(ctx, point_count + 2, "construct intersection chart knots")?;
    let knots = std::iter::once(first)
        .chain(parameters.iter().copied())
        .chain(std::iter::once(last))
        .collect();
    charge_items(ctx, point_count, "construct intersection curve controls")?;
    let controls = points
        .iter()
        .map(|p| Point3::new(p[0] * LEN_TO_MM, p[1] * LEN_TO_MM, p[2] * LEN_TO_MM))
        .collect();
    charge_items(ctx, point_count, "admit intersection curve poles")?;
    let nurbs = match NurbsCurve::from_lanes(1, knots, controls, None, false) {
        Ok(nurbs) => nurbs,
        Err(error) => {
            refusal.note(
                format_args!("sldprt intersection chart curve for record attr {record_attr}"),
                &error,
            );
            return Ok(None);
        }
    };
    Ok(Some((
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
        parameters,
        reversed,
    )))
}

fn solved_support_uv(
    ctx: Option<&DecodeContext<'_>>,
    parameters: &[f64],
    reversed: bool,
    records: Option<&[UvRecord]>,
) -> Result<Option<[Vec<Point2>; 2]>, CodecError> {
    let Some(expected_values) = parameters.len().checked_mul(4) else {
        return Ok(None);
    };
    let Some(records) = records else {
        return Ok(None);
    };
    let mut candidate: Option<[Vec<Point2>; 2]> = None;
    for record in records {
        if record.width != UvWidth::Four || record.values.len() != expected_values {
            continue;
        }
        charge_items(
            ctx,
            parameters.len() * 2,
            "construct intersection support UV controls",
        )?;
        let controls = [0usize, 1].map(|support| {
            let mut control_points = record
                .values
                .chunks_exact(4)
                .map(|row| Point2::new(row[support * 2], row[support * 2 + 1]))
                .collect::<Vec<_>>();
            if reversed {
                control_points.reverse();
            }
            control_points
        });
        if candidate
            .as_ref()
            .is_some_and(|previous| previous != &controls)
        {
            return Ok(None);
        }
        candidate = Some(controls);
    }
    Ok(candidate)
}

fn nearest_term(
    records: &HashMap<u16, Vec<[f64; 3]>>,
    attr: u16,
    endpoint: [f64; 3],
) -> Option<([f64; 3], f64)> {
    records
        .get(&attr)?
        .iter()
        .copied()
        .fold(None, |best, point| {
            let candidate = (point, distance(point, endpoint));
            match best {
                Some(best) if best.1 <= candidate.1 => Some(best),
                _ => Some(candidate),
            }
        })
}

/// Scan intersection carriers whose referenced chart and terminators resolve,
/// keyed by carrier attribute.
///
/// The composite body is `attr:u16 ordinal:u32 refs:u16[5] marker:u8(0x2b|0x2d)`
/// then six payload references `[support0, support1, chart, term_start,
/// term_end, uv]`. Terminators replace the approximate chart endpoints. When
/// both 24-byte and 88-byte chart strides frame, the referenced terminators
/// select the unique candidate with the least endpoint displacement. An absent
/// or inconsistent optional UV record does not invalidate the model-space
/// curve; only a unique complete width-4 record supplies solved pcurves.
pub(super) fn scan_intersection_carriers(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    lane_refusals: &mut Vec<LossNote>,
) -> Result<HashMap<u16, IntersectionCarrier>, CodecError> {
    let charts = chart_records(ctx, bytes)?;
    let terms = term_records(ctx, bytes)?;
    let uvs = uv_records(ctx, bytes)?;
    if charts.is_empty() || terms.is_empty() {
        return Ok(HashMap::new());
    }
    let mut out = HashMap::new();
    for (offset, body, _) in composite_records(bytes) {
        let Some(attr) = View::u16_be_at(bytes, body + isect::ATTR) else {
            continue;
        };
        let payload = body + isect::PAYLOAD;
        let mut refs = [0_u16; 6];
        let mut valid = true;
        for (index, reference) in refs.iter_mut().enumerate() {
            if let Some(value) = View::u16_be_at(bytes, payload + index * 2) {
                *reference = value;
            } else {
                valid = false;
                break;
            }
        }
        if !valid {
            continue;
        }
        let (chart_ref, start_ref, end_ref, uv_ref) = (refs[2], refs[3], refs[4], refs[5]);
        let Some(candidates) = charts.get(&chart_ref) else {
            continue;
        };
        let mut chart_refusal = crate::lane_refusal::LaneRefusals::new();
        let mut selected: Option<SolvedChart> = None;
        let mut ambiguous = false;
        for chart in candidates {
            let [first, last] = chart.endpoints;
            let Some((start, start_distance)) = nearest_term(&terms, start_ref, first) else {
                continue;
            };
            let Some((end, end_distance)) = nearest_term(&terms, end_ref, last) else {
                continue;
            };
            let endpoint_displacement = start_distance + end_distance;
            let Some((geometry, parameters, reversed)) =
                solved_curve(ctx, chart, start, end, attr, &mut chart_refusal)?
            else {
                continue;
            };
            let fit_tolerance_mm = chart.chordal_error * LEN_TO_MM;
            if !fit_tolerance_mm.is_finite() {
                continue;
            }
            let candidate = SolvedChart {
                geometry,
                parameters,
                fit_tolerance_mm,
                reversed,
                endpoint_displacement,
            };
            match selected.as_ref() {
                None => selected = Some(candidate),
                Some(previous) => match candidate
                    .endpoint_displacement
                    .total_cmp(&previous.endpoint_displacement)
                {
                    std::cmp::Ordering::Less => {
                        selected = Some(candidate);
                        ambiguous = false;
                    }
                    std::cmp::Ordering::Equal => ambiguous = true,
                    std::cmp::Ordering::Greater => {}
                },
            }
        }
        lane_refusals.extend(chart_refusal.take_records().into_iter().map(|record| {
            crate::loss::spline_lane_refusal(&format!(
                "intersection chart for attr {attr}: {record}"
            ))
        }));
        let Some(selected) = selected else {
            continue;
        };
        if ambiguous {
            continue;
        }
        let supports = [refs[0], refs[1]];
        let support_uv = solved_support_uv(
            ctx,
            &selected.parameters,
            selected.reversed,
            uvs.get(&uv_ref).map(Vec::as_slice),
        )?;
        if let Some(ctx) = ctx {
            if !out.contains_key(&attr) {
                ctx.charge_collection_items(1, "collect Parasolid intersection carriers")?;
            }
        }
        out.entry(attr).or_insert(IntersectionCarrier {
            carrier: CurveCarrier {
                attr,
                offset,
                end: body + isect::LEN,
                geometry: selected.geometry,
                parameter_range: None,
            },
            support_data: IntersectionSupportData {
                supports,
                fit_tolerance_mm: selected.fit_tolerance_mm,
                support_uv,
            },
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {

    #[test]
    fn numerical_ranges_short_chart_chord_preserves_parameter_increment() {
        let endpoints = [[0., 0., 0.], [1e-200, 0., 0.]];
        let chart = super::Chart {
            endpoints,
            interior_points: Vec::new(),
            base_parameter: 0.,
            base_scale: 1e200,
            chordal_error: 1e-210,
        };
        let (_, parameters, reversed) = super::solved_curve(
            None,
            &chart,
            endpoints[0],
            endpoints[1],
            0,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("curve admission")
        .unwrap();
        assert_eq!(parameters, vec![0., 1.]);
        assert!(!reversed);
    }

    use super::super::LEN_TO_MM;
    use super::{
        chart_candidates, chart_records, scan_intersection_carriers, term_records, uv_at,
        uv_records, UvWidth, MISSING_PARAMETER,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::geometry::CurveGeometry;
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::math::Point3;

    const POINTS: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.01, 0.01, 0.0]];

    fn chart(attr: u16, points: &[[f64; 3]]) -> Vec<u8> {
        let mut bytes = vec![0, 0x28];
        bytes.extend_from_slice(&(points.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&0.0f64.to_be_bytes());
        bytes.extend_from_slice(&1.0f64.to_be_bytes());
        bytes.extend_from_slice(&(points.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&1e-5f64.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        bytes.extend_from_slice(&MISSING_PARAMETER.to_be_bytes());
        bytes.extend_from_slice(&MISSING_PARAMETER.to_be_bytes());
        for point in points {
            for value in point {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
        }
        bytes
    }

    fn extended_chart(attr: u16, points: &[[f64; 3]], tangent: [f64; 3]) -> Vec<u8> {
        let mut bytes = vec![0, 0x28];
        bytes.extend_from_slice(&(points.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&0.0f64.to_be_bytes());
        bytes.extend_from_slice(&1.0f64.to_be_bytes());
        bytes.extend_from_slice(&(points.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&1e-5f64.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        bytes.extend_from_slice(&MISSING_PARAMETER.to_be_bytes());
        bytes.extend_from_slice(&MISSING_PARAMETER.to_be_bytes());
        for point in points {
            for value in point {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            bytes.extend_from_slice(&[0u8; 32]);
            for value in tangent {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            bytes.extend_from_slice(&[0u8; 8]);
        }
        bytes
    }

    fn term(attr: u16, point: [f64; 3]) -> Vec<u8> {
        let mut bytes = vec![0, 0x29];
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(b"L?");
        for value in point {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    fn uv(attr: u16, rows: usize) -> Vec<u8> {
        let mut bytes = vec![0, 0xcc];
        bytes.extend_from_slice(&((rows * 4) as u32).to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.push(4);
        for index in 0..rows * 4 {
            bytes.extend_from_slice(&(index as f64).to_be_bytes());
        }
        bytes
    }

    fn composite(attr: u16, payload: [u16; 6]) -> Vec<u8> {
        let mut bytes = vec![0, 0x26];
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 4]);
        bytes.extend_from_slice(&[0u8; 10]);
        bytes.push(0x2b);
        for reference in payload {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes
    }

    fn intersection_data(attr: u16, payload: [u16; 6]) -> Vec<u8> {
        let mut bytes = vec![0, 1, 0x5a];
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 4]);
        bytes.extend_from_slice(&[0u8; 10]);
        bytes.push(0x2b);
        for reference in payload {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes
    }

    fn stream() -> Vec<u8> {
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));
        bytes
    }

    macro_rules! intersection_collection_boundary {
        ($name:ident, $fixture:expr, $route:ident, $cap:expr, $operation:literal) => {
            #[test]
            fn $name() {
                let bytes = $fixture;
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = $cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                let error = match $route(Some(&ctx), &bytes) {
                    Err(error) => error,
                    Ok(_) => panic!("record collection exceeded its limit"),
                };
                assert!(matches!(error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::CollectionItems
                            && limit.operation == $operation), "{error:?}");
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(
                    &bytes,
                    &arena,
                    &DecodePolicy::service(),
                )
                .expect("root");
                assert!($route(Some(&ctx), &bytes).is_ok());
            }
        };
    }

    intersection_collection_boundary!(
        parasolid_chart_interior_points_refuse_before_allocation,
        chart(4, &POINTS),
        chart_records,
        0,
        "decode Parasolid chart interior points"
    );
    intersection_collection_boundary!(
        parasolid_chart_stride_candidates_refuse_before_collection,
        chart(4, &POINTS),
        chart_records,
        1,
        "collect Parasolid chart stride candidates"
    );
    intersection_collection_boundary!(
        parasolid_chart_groups_refuse_before_collection,
        chart(4, &POINTS),
        chart_records,
        2,
        "collect Parasolid intersection charts"
    );
    intersection_collection_boundary!(
        parasolid_chart_candidates_refuse_before_collection,
        chart(4, &POINTS),
        chart_records,
        3,
        "collect Parasolid intersection chart candidates"
    );
    intersection_collection_boundary!(
        parasolid_terminator_groups_refuse_before_collection,
        term(5, POINTS[0]),
        term_records,
        0,
        "collect Parasolid terminator groups"
    );
    intersection_collection_boundary!(
        parasolid_terminator_points_refuse_before_collection,
        term(5, POINTS[0]),
        term_records,
        1,
        "collect Parasolid terminator points"
    );
    intersection_collection_boundary!(
        parasolid_support_uv_values_refuse_before_allocation,
        uv(7, POINTS.len()),
        uv_records,
        11,
        "decode Parasolid support UV values"
    );
    intersection_collection_boundary!(
        parasolid_support_uv_groups_refuse_before_collection,
        uv(7, POINTS.len()),
        uv_records,
        12,
        "collect Parasolid support UV groups"
    );
    intersection_collection_boundary!(
        parasolid_support_uv_records_refuse_before_collection,
        uv(7, POINTS.len()),
        uv_records,
        13,
        "collect Parasolid support UV records"
    );

    fn intersection_curve_boundary(cap: u64, operation: &'static str) {
        let bytes = [0_u8; 1];
        let chart = super::Chart {
            endpoints: [POINTS[0], POINTS[2]],
            interior_points: vec![POINTS[1]],
            base_parameter: 0.0,
            base_scale: 1.0,
            chordal_error: 0.00001,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = match super::solved_curve(
            Some(&ctx),
            &chart,
            POINTS[0],
            POINTS[2],
            9,
            &mut crate::lane_refusal::LaneRefusals::new(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("curve construction exceeded its limit"),
        };
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation),
            "{error:?}"
        );
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(super::solved_curve(
            Some(&ctx),
            &chart,
            POINTS[0],
            POINTS[2],
            9,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("service curve")
        .is_some());
    }

    #[test]
    fn parasolid_intersection_parameters_refuse_before_allocation() {
        intersection_curve_boundary(2, "construct intersection chart parameters");
    }

    #[test]
    fn parasolid_intersection_points_refuse_before_allocation() {
        intersection_curve_boundary(5, "construct intersection chart points");
    }

    #[test]
    fn parasolid_intersection_knots_refuse_before_allocation() {
        intersection_curve_boundary(10, "construct intersection chart knots");
    }

    #[test]
    fn parasolid_intersection_controls_refuse_before_allocation() {
        intersection_curve_boundary(13, "construct intersection curve controls");
    }

    #[test]
    fn parasolid_intersection_poles_refuse_before_admission() {
        intersection_curve_boundary(16, "admit intersection curve poles");
    }

    #[test]
    fn parasolid_intersection_support_uv_controls_refuse_before_allocation() {
        let bytes = [0_u8; 1];
        let records = [super::UvRecord {
            width: UvWidth::Four,
            values: vec![0.0; 12],
        }];
        let parameters = [0.0, 0.5, 1.0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 5;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = match super::solved_support_uv(Some(&ctx), &parameters, false, Some(&records)) {
            Err(error) => error,
            Ok(_) => panic!("six controls exceed five items"),
        };
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "construct intersection support UV controls"));
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            super::solved_support_uv(Some(&ctx), &parameters, false, Some(&records))
                .expect("service UV")
                .map(|controls| [controls[0].len(), controls[1].len()]),
            Some([3, 3])
        );
    }

    #[test]
    fn parasolid_intersection_carriers_refuse_before_collection() {
        let bytes = stream();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 46;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = match scan_intersection_carriers(Some(&ctx), &bytes, &mut Vec::new()) {
            Err(error) => error,
            Ok(_) => panic!("carrier insertion exceeds the collection limit"),
        };
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "collect Parasolid intersection carriers"),
            "{error:?}"
        );
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(
            scan_intersection_carriers(Some(&ctx), &bytes, &mut Vec::new())
                .expect("service scan")
                .contains_key(&9)
        );
    }

    #[test]
    fn marker_three_uv_has_two_values_per_chart_point() {
        let points = (0..9)
            .map(|index| [f64::from(index) * 0.01, 0.0, 0.0])
            .collect::<Vec<_>>();
        let mut record = uv(7, points.len());
        record
            .get_mut(2..6)
            .unwrap()
            .copy_from_slice(&18_u32.to_be_bytes());
        *record.get_mut(8).unwrap() = 3;
        record.truncate(9 + 18 * 8);
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &points));
        bytes.extend(term(5, *points.first().unwrap()));
        bytes.extend(term(6, *points.last().unwrap()));
        bytes.extend(record);
        let charts = chart_records(None, &bytes).expect("chart scan");
        let records = uv_records(None, &bytes).expect("UV scan");
        let chart = &charts[&4][0];
        let uv = &records[&7][0];
        assert_eq!(chart.interior_points.len() + 2, 9);
        assert!(uv.width == UvWidth::Two);
        assert_eq!(uv.values.len(), (chart.interior_points.len() + 2) * 2);
        assert!(scan_intersection_carriers(None, &bytes, &mut Vec::new())
            .expect("intersection scan")
            .contains_key(&9));
    }

    #[test]
    fn width_two_uv_is_legal_without_a_paired_support_cache() {
        let mut record = vec![0, 0xcc];
        record.extend_from_slice(&6_u32.to_be_bytes());
        record.extend_from_slice(&7_u16.to_be_bytes());
        record.push(2);
        for value in [0.0_f64, 1.0, 2.0, 3.0, 4.0, 5.0] {
            record.extend_from_slice(&value.to_be_bytes());
        }
        assert!(uv_at(None, &record, 2).expect("UV parse").is_some());
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(record);
        let carriers =
            scan_intersection_carriers(None, &bytes, &mut Vec::new()).expect("intersection scan");
        let carrier = carriers
            .get(&9)
            .expect("width-two support leaves the curve available");
        assert!(carrier.support_data.support_uv.is_none());
    }

    #[test]
    fn consistent_composite_yields_polyline() {
        let carriers = scan_intersection_carriers(None, &stream(), &mut Vec::new())
            .expect("intersection scan");
        let carrier = carriers.get(&9).expect("composite decoded");
        let Some(SolvedCurveGeometry::Nurbs(curve)) = carrier.carrier.geometry.solved() else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.degree(), 1);
        assert_eq!(curve.control_points().len(), 3);
        assert_eq!(curve.control_points()[1], Point3::new(10.0, 0.0, 0.0));
        assert_eq!(curve.knots().len(), 5);
        assert!((curve.knots()[2] - 0.01).abs() < 1.0e-12);
        assert!((curve.knots()[3] - 0.02).abs() < 1.0e-12);
        let support_data = &carrier.support_data;
        assert_eq!(support_data.supports, [2, 3]);
        let support_uv = support_data
            .support_uv
            .as_ref()
            .expect("width-four UV cache");
        assert_eq!(
            support_uv[0],
            &[
                Point2::new(0.0, 1.0),
                Point2::new(4.0, 5.0),
                Point2::new(8.0, 9.0)
            ]
        );
        assert_eq!(support_data.fit_tolerance_mm, 0.01);
    }

    #[test]
    fn intersection_data_entity_uses_the_same_composite_payload() {
        let mut bytes = intersection_data(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));

        let carrier = scan_intersection_carriers(None, &bytes, &mut Vec::new())
            .expect("intersection scan")
            .remove(&9)
            .expect("intersection-data entity decoded");
        assert_eq!(carrier.carrier.offset, 0);
        let Some(SolvedCurveGeometry::Nurbs(curve)) = carrier.carrier.geometry.solved() else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.control_points().len(), POINTS.len());
    }

    #[test]
    fn negative_chart_scale_reverses_curve_and_uv_caches_atomically() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        let mut chart = chart(4, &POINTS);
        chart[16..24].copy_from_slice(&(-1.0f64).to_be_bytes());
        bytes.extend(chart);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));

        let carriers =
            scan_intersection_carriers(None, &bytes, &mut Vec::new()).expect("intersection scan");
        let carrier = carriers.get(&9).expect("decreasing chart decoded");
        let Some(SolvedCurveGeometry::Nurbs(curve)) = carrier.carrier.geometry.solved() else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.knots().as_slice(), [-0.02, -0.02, -0.01, 0.0, 0.0]);
        assert_eq!(
            curve.control_points(),
            [
                Point3::new(10.0, 10.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ]
        );
        let support_data = &carrier.support_data;
        let control_points = &support_data.support_uv.as_ref().expect("UV cache")[0];
        assert_eq!(control_points[0], Point2::new(8.0, 9.0));
        assert_eq!(control_points[2], Point2::new(0.0, 1.0));
        assert_eq!(curve.knots().as_slice(), [-0.02, -0.02, -0.01, 0.0, 0.0]);
    }

    #[test]
    fn seam_row_uv_count_is_accepted() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len() + 1));
        let carriers =
            scan_intersection_carriers(None, &bytes, &mut Vec::new()).expect("intersection scan");
        let carrier = carriers.get(&9).expect("seam-row carrier decoded");
        assert!(carrier.support_data.support_uv.is_none());
    }

    #[test]
    fn exact_terminator_replaces_an_approximate_chart_endpoint() {
        let end = [0.011, 0.01, 0.0];
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, end));
        bytes.extend(uv(7, POINTS.len()));
        let carriers =
            scan_intersection_carriers(None, &bytes, &mut Vec::new()).expect("intersection scan");
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &carriers
            .get(&9)
            .expect("composite decoded")
            .carrier
            .geometry
        else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(
            *curve.control_points().last().expect("points"),
            Point3::new(end[0] * LEN_TO_MM, end[1] * LEN_TO_MM, end[2] * LEN_TO_MM),
        );
    }

    #[test]
    fn missing_chart_sentinels_reject_the_chart() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        let mut bad = chart(4, &POINTS);
        let at = bad.len() - POINTS.len() * 24 - 16;
        bad[at..at + 8].copy_from_slice(&0.0f64.to_be_bytes());
        bytes.extend(bad);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));
        assert!(scan_intersection_carriers(None, &bytes, &mut Vec::new())
            .expect("intersection scan")
            .is_empty());
    }

    #[test]
    fn ring_composite_with_one_char_label_and_no_uv_record_decodes() {
        let ring = [
            [0.0, 0.0, 0.0],
            [0.01, 0.0, 0.0],
            [0.01, 0.01, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let mut bytes = composite(9, [2, 3, 4, 5, 5, 6]);
        bytes.extend(chart(4, &ring));
        let mut term = vec![0u8, 0x29];
        term.extend_from_slice(&1u32.to_be_bytes());
        term.extend_from_slice(&5u16.to_be_bytes());
        term.push(b'H');
        for value in ring[0] {
            term.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend(term);
        let carriers =
            scan_intersection_carriers(None, &bytes, &mut Vec::new()).expect("intersection scan");
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &carriers
            .get(&9)
            .expect("ring composite decoded")
            .carrier
            .geometry
        else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.control_points().len(), 4);
        assert_eq!(curve.control_points()[0], curve.control_points()[3]);
    }

    #[test]
    fn mismatched_optional_uv_count_does_not_reject_the_curve() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len() + 2));
        let carriers =
            scan_intersection_carriers(None, &bytes, &mut Vec::new()).expect("intersection scan");
        assert!(carriers.contains_key(&9));
        assert!(carriers[&9].support_data.support_uv.is_none());
    }

    #[test]
    fn extended_chart_stride_is_selected_by_witnesses() {
        let chart_bytes = extended_chart(4, &POINTS, [0.5, 0.0, 0.0]);
        let (_, candidates) = chart_candidates(None, &chart_bytes, 2)
            .expect("chart parse")
            .expect("chart candidates");
        assert_eq!(candidates.len(), 2);

        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart_bytes);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));

        let carriers =
            scan_intersection_carriers(None, &bytes, &mut Vec::new()).expect("intersection scan");
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &carriers
            .get(&9)
            .expect("extended chart decoded")
            .carrier
            .geometry
        else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.control_points().len(), POINTS.len());
        assert_eq!(
            *curve.control_points().last().expect("points"),
            Point3::new(
                POINTS[2][0] * LEN_TO_MM,
                POINTS[2][1] * LEN_TO_MM,
                POINTS[2][2] * LEN_TO_MM,
            ),
        );
    }

    #[test]
    fn ambiguous_chart_stride_does_not_select_first_candidate() {
        let end = POINTS[2];
        let mut chart_bytes = extended_chart(4, &[POINTS[0], end], [0.5, 0.0, 0.0]);
        let bare_endpoint = 2 + 6 + 52 + 24;
        for (index, value) in end.into_iter().enumerate() {
            chart_bytes[bare_endpoint + index * 8..bare_endpoint + (index + 1) * 8]
                .copy_from_slice(&value.to_be_bytes());
        }
        let (_, candidates) = chart_candidates(None, &chart_bytes, 2)
            .expect("chart parse")
            .expect("chart candidates");
        assert_eq!(candidates.len(), 2);

        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart_bytes);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, end));
        bytes.extend(uv(7, 2));
        assert!(scan_intersection_carriers(None, &bytes, &mut Vec::new())
            .expect("intersection scan")
            .is_empty());
    }
}
