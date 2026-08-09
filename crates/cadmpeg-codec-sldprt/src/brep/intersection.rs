// SPDX-License-Identifier: Apache-2.0
//! Surface-intersection curve carriers.
//!
//! A `00 26` composite record or `00 01 5a` intersection-data entity carries a
//! curve defined by the intersection of two support surfaces. Its payload
//! references a `00 28` chart record (the solved point cache), two `00 29`
//! terminator records (the exact curve endpoints), and a `00 cc` support-UV
//! record. A carrier whose referenced chart and terminators are mutually
//! consistent yields a derived degree-one NURBS curve through the chart points
//! with the terminators as endpoints. A complete width-4 UV record additionally
//! yields co-parameterized support pcurve caches.

use std::collections::HashMap;

use cadmpeg_core::be::{f64_at, u16_at, u32_at};
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, PcurveGeometry};
use cadmpeg_ir::math::{Point2, Point3};

use super::{Carrier, CarrierGeometry, LEN_TO_MM};

/// Chart parameter sentinel marking an absent value.
const MISSING_PARAMETER: f64 = -31_415_800_000_000.0;
/// Fixed bytes between an inline `term_use` label and its terminator body.
const INLINE_TERM_TAIL: &[u8] = b"\x00\x00\x00\x01\x01\x63\x43\x5a";
/// Fixed bytes between an inline `values` label and its support-UV body.
const INLINE_UV_TAIL: &[u8] = b"\x00\x00\x00\x02\x01\x66\x01";

/// One decoded chart: solved points in metres and parameter bookkeeping.
struct Chart {
    points: Vec<[f64; 3]>,
    base_parameter: f64,
    base_scale: f64,
    chordal_error: f64,
}

/// One validated intersection curve and its optional support parameterization.
pub(super) struct IntersectionCarrier {
    pub carrier: Carrier,
    pub parameterization: Option<IntersectionParameterization>,
}

/// Ordered support surfaces and their co-parameterized solved UV caches.
#[derive(Clone)]
pub(super) struct IntersectionParameterization {
    pub supports: [u16; 2],
    pub pcurves: [PcurveGeometry; 2],
    pub parameter_range: [f64; 2],
    pub model_points: Vec<Point3>,
    pub fit_tolerance_mm: f64,
}

struct UvRecord {
    width: usize,
    values: Vec<f64>,
}

/// Offsets of every `00 tt` tag, with the optional `0xff` escape skipped.
fn record_bodies(bytes: &[u8], tt: u8) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 2 <= bytes.len() {
        if bytes[at] == 0x00 && bytes[at + 1] == tt {
            let mut body = at + 2;
            if bytes.get(body) == Some(&0xff) {
                body += 1;
            }
            out.push(body);
        }
        at += 1;
    }
    out
}

/// Carrier, body, and payload-marker offsets for both intersection forms.
fn composite_records(bytes: &[u8]) -> Vec<(usize, usize, usize)> {
    let mut records = record_bodies(bytes, 0x26)
        .into_iter()
        .filter_map(|body| {
            let marker = body.checked_add(16)?;
            matches!(bytes.get(marker), Some(0x2b | 0x2d)).then_some((body - 2, body, marker))
        })
        .collect::<Vec<_>>();

    for offset in 0..bytes.len().saturating_sub(20) {
        if bytes.get(offset..offset + 3) != Some(&[0x00, 0x01, 0x5a]) {
            continue;
        }
        let body = offset + 3;
        let marker = body + 16;
        if matches!(bytes.get(marker), Some(0x2b | 0x2d)) {
            records.push((offset, body, marker));
        }
    }
    records
}

fn finite_point(bytes: &[u8], at: usize) -> Option<[f64; 3]> {
    let point = [
        f64_at(bytes, at)?,
        f64_at(bytes, at + 8)?,
        f64_at(bytes, at + 16)?,
    ];
    point
        .iter()
        .all(|value| value.is_finite() && value.abs() < 1e6)
        .then_some(point)
}

fn finite_tangent(bytes: &[u8], at: usize) -> bool {
    let Some(tangent) = (|| {
        Some([
            f64_at(bytes, at)?,
            f64_at(bytes, at + 8)?,
            f64_at(bytes, at + 16)?,
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
fn chart_records(bytes: &[u8]) -> HashMap<u16, Vec<Chart>> {
    let mut out: HashMap<u16, Vec<Chart>> = HashMap::new();
    for body in record_bodies(bytes, 0x28) {
        let Some((attr, candidates)) = chart_candidates(bytes, body) else {
            continue;
        };
        out.entry(attr).or_default().extend(candidates);
    }
    out
}

fn chart_candidates(bytes: &[u8], body: usize) -> Option<(u16, Vec<Chart>)> {
    let count = u32_at(bytes, body)? as usize;
    let attr = u16_at(bytes, body + 4)?;
    let preamble = body + 6;
    let base_parameter = f64_at(bytes, preamble)?;
    let base_scale = f64_at(bytes, preamble + 8)?;
    let chart_count = u32_at(bytes, preamble + 16)? as usize;
    let chordal_error = f64_at(bytes, preamble + 20)?;
    if !(2..=4096).contains(&count)
        || chart_count != count
        || !base_parameter.is_finite()
        || !base_scale.is_finite()
        || base_scale == 0.0
        || !chordal_error.is_finite()
        || chordal_error <= 0.0
        || f64_at(bytes, preamble + 36) != Some(MISSING_PARAMETER)
        || f64_at(bytes, preamble + 44) != Some(MISSING_PARAMETER)
    {
        return None;
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
        let Some(points) = (0..count)
            .map(|index| finite_point(bytes, block + index * stride))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !extended && points.windows(2).all(|pair| pair[0] == pair[1]) {
            continue;
        }
        candidates.push(Chart {
            points,
            base_parameter,
            base_scale,
            chordal_error,
        });
    }
    (!candidates.is_empty()).then_some((attr, candidates))
}

/// Parse a terminator body: `count:u32 attr:u16`, a kind label, then the
/// endpoint. The label is one kind character (`L` limit, `H` ring, `T`
/// terminator) with an optional second character (`?`, `F`, or `S`). Both
/// label widths yield a candidate endpoint; composite validation selects the
/// candidate that matches the chart.
fn term_at(bytes: &[u8], body: usize, out: &mut HashMap<u16, Vec<[f64; 3]>>) {
    let (Some(count), Some(attr)) = (u32_at(bytes, body), u16_at(bytes, body + 4)) else {
        return;
    };
    if !(1..=2).contains(&count) {
        return;
    }
    if !matches!(bytes.get(body + 6), Some(b'L' | b'H' | b'T')) {
        return;
    }
    let two_char = matches!(bytes.get(body + 7), Some(b'?' | b'F' | b'S'));
    for label_len in [2usize, 1] {
        if label_len == 2 && !two_char {
            continue;
        }
        if let Some(point) = finite_point(bytes, body + 6 + label_len) {
            out.entry(attr).or_default().push(point);
        }
    }
}

/// Every `00 29` or inline `term_use` terminator, keyed by attribute.
fn term_records(bytes: &[u8]) -> HashMap<u16, Vec<[f64; 3]>> {
    let mut out: HashMap<u16, Vec<[f64; 3]>> = HashMap::new();
    for body in record_bodies(bytes, 0x29) {
        term_at(bytes, body, &mut out);
    }
    for label in find_bytes(bytes, b"term_use") {
        let tail = label + b"term_use".len();
        if bytes.get(tail..tail + INLINE_TERM_TAIL.len()) == Some(INLINE_TERM_TAIL) {
            term_at(bytes, tail + INLINE_TERM_TAIL.len(), &mut out);
        }
    }
    out
}

/// Parse a support-UV body: `count:u32 attr:u16 width_marker:u8(2|3|4)` then
/// `count` finite f64 values.
fn uv_at(bytes: &[u8], body: usize) -> Option<(u16, UvRecord)> {
    let count = u32_at(bytes, body)? as usize;
    let attr = u16_at(bytes, body + 4)?;
    let marker = *bytes.get(body + 6)?;
    if !(2..=4).contains(&marker) {
        return None;
    }
    let width = if marker == 4 { 4 } else { 2 };
    if count < width * 2 || !count.is_multiple_of(width) {
        return None;
    }
    let values = (0..count)
        .map(|index| f64_at(bytes, body + 7 + index * 8))
        .collect::<Option<Vec<_>>>()?;
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some((attr, UvRecord { width, values }))
}

/// Every `00 cc` or inline `values` support-UV record, keyed by attribute.
fn uv_records(bytes: &[u8]) -> HashMap<u16, Vec<UvRecord>> {
    let mut out: HashMap<u16, Vec<UvRecord>> = HashMap::new();
    for body in record_bodies(bytes, 0xcc) {
        if let Some((attr, shape)) = uv_at(bytes, body) {
            out.entry(attr).or_default().push(shape);
        }
    }
    for label in find_bytes(bytes, b"values") {
        let tail = label + b"values".len();
        if bytes.get(tail..tail + INLINE_UV_TAIL.len()) == Some(INLINE_UV_TAIL) {
            if let Some((attr, shape)) = uv_at(bytes, tail + INLINE_UV_TAIL.len()) {
                out.entry(attr).or_default().push(shape);
            }
        }
    }
    out
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(&b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

fn chart_parameters(chart: &Chart, points: &[[f64; 3]]) -> Vec<f64> {
    let mut parameters = Vec::with_capacity(points.len());
    parameters.push(chart.base_parameter);
    for pair in points.windows(2) {
        let previous = *parameters.last().expect("base parameter inserted");
        parameters.push(previous + distance(pair[0], pair[1]) * chart.base_scale);
    }
    parameters
}

fn degree_one_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.push(parameters[0]);
    knots.extend_from_slice(parameters);
    knots.push(*parameters.last().expect("non-empty parameters"));
    knots
}

/// Build the derived polyline curve for one validated composite.
fn solved_curve(
    chart: &Chart,
    start: [f64; 3],
    end: [f64; 3],
) -> Option<(CurveGeometry, Vec<f64>, bool)> {
    let mut parameters = chart_parameters(chart, &chart.points);
    let mut points = chart.points.clone();
    *points.first_mut().expect("chart has at least two points") = start;
    *points.last_mut().expect("chart has at least two points") = end;
    let reversed = if parameters.windows(2).all(|pair| pair[0] < pair[1]) {
        false
    } else if parameters.windows(2).all(|pair| pair[0] > pair[1]) {
        parameters.reverse();
        points.reverse();
        true
    } else {
        return None;
    };
    let knots = degree_one_knots(&parameters);
    Some((
        CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots,
            control_points: points
                .iter()
                .map(|p| Point3::new(p[0] * LEN_TO_MM, p[1] * LEN_TO_MM, p[2] * LEN_TO_MM))
                .collect(),
            weights: None,
            periodic: false,
        }),
        parameters,
        reversed,
    ))
}

fn solved_parameterization(
    supports: [u16; 2],
    parameters: &[f64],
    model_points: &[Point3],
    fit_tolerance_mm: f64,
    reversed: bool,
    records: Option<&[UvRecord]>,
) -> Option<IntersectionParameterization> {
    let expected_values = parameters.len().checked_mul(4)?;
    let mut candidates = records?
        .iter()
        .filter(|record| record.width == 4 && record.values.len() == expected_values)
        .map(|record| {
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
            let knots = degree_one_knots(parameters);
            IntersectionParameterization {
                supports,
                pcurves: controls.map(|control_points| PcurveGeometry::Nurbs {
                    degree: 1,
                    knots: knots.clone(),
                    control_points,
                    weights: None,
                    periodic: false,
                }),
                parameter_range: [
                    parameters[0],
                    *parameters.last().expect("non-empty parameters"),
                ],
                model_points: model_points.to_vec(),
                fit_tolerance_mm,
            }
        });
    let candidate = candidates.next()?;
    candidates
        .all(|other| {
            other.supports == candidate.supports
                && other.pcurves == candidate.pcurves
                && other.parameter_range == candidate.parameter_range
                && other.model_points == candidate.model_points
                && other.fit_tolerance_mm == candidate.fit_tolerance_mm
        })
        .then_some(candidate)
}

/// Scan intersection carriers whose chart and terminator witnesses are mutually
/// consistent, keyed by carrier attribute.
///
/// The composite body is `attr:u16 ordinal:u32 refs:u16[5] marker:u8(0x2b|0x2d)`
/// then six payload references `[support0, support1, chart, term_start,
/// term_end, uv]`. Both terminators must sit within the chart chordal error of
/// the corresponding chart endpoint (a ring names one terminator twice). When
/// the composite's UV reference resolves, at least one record must cover the
/// chart points, with an optional extra row at a periodic seam.
pub(super) fn scan_intersection_carriers(bytes: &[u8]) -> HashMap<u16, IntersectionCarrier> {
    let charts = chart_records(bytes);
    let terms = term_records(bytes);
    let uvs = uv_records(bytes);
    if charts.is_empty() || terms.is_empty() {
        return HashMap::new();
    }
    let mut out = HashMap::new();
    for (offset, body, marker_at) in composite_records(bytes) {
        let Some(attr) = u16_at(bytes, body) else {
            continue;
        };
        let payload = marker_at + 1;
        let Some(refs) = (0..6)
            .map(|index| u16_at(bytes, payload + index * 2))
            .collect::<Option<Vec<u16>>>()
        else {
            continue;
        };
        let (chart_ref, start_ref, end_ref, uv_ref) = (refs[2], refs[3], refs[4], refs[5]);
        let Some(candidates) = charts.get(&chart_ref) else {
            continue;
        };
        let mut matches = candidates.iter().filter_map(|chart| {
            let first = *chart.points.first()?;
            let last = *chart.points.last()?;
            let start = terms
                .get(&start_ref)?
                .iter()
                .find(|point| distance(**point, first) <= chart.chordal_error)?;
            let end = terms
                .get(&end_ref)?
                .iter()
                .find(|point| distance(**point, last) <= chart.chordal_error)?;
            let n = chart.points.len();
            if !uvs.get(&uv_ref).is_none_or(|records| {
                records.iter().any(|record| {
                    record.values.len() == record.width * n
                        || record.values.len() == record.width * (n + 1)
                })
            }) {
                return None;
            }
            let (geometry, parameters, reversed) = solved_curve(chart, *start, *end)?;
            let fit_tolerance_mm = chart.chordal_error * LEN_TO_MM;
            fit_tolerance_mm.is_finite().then_some((
                geometry,
                parameters,
                fit_tolerance_mm,
                reversed,
            ))
        });
        let Some((geometry, parameters, fit_tolerance_mm, reversed)) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let supports = [refs[0], refs[1]];
        let model_points = match &geometry {
            CurveGeometry::Nurbs(curve) => curve.control_points.as_slice(),
            _ => unreachable!("solved intersection curve is a NURBS cache"),
        };
        let parameterization = solved_parameterization(
            supports,
            &parameters,
            model_points,
            fit_tolerance_mm,
            reversed,
            uvs.get(&uv_ref).map(Vec::as_slice),
        );
        out.entry(attr).or_insert(IntersectionCarrier {
            carrier: Carrier {
                attr,
                offset,
                end: payload + 12,
                geometry: CarrierGeometry::Curve(geometry),
                frame: None,
                parameter_range: None,
                orientation_reversed: false,
            },
            parameterization,
        });
    }
    out
}

/// Offsets of every occurrence of `needle`.
fn find_bytes(bytes: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || bytes.len() < needle.len() {
        return Vec::new();
    }
    (0..=bytes.len() - needle.len())
        .filter(|&at| &bytes[at..at + needle.len()] == needle)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn consistent_composite_yields_polyline() {
        let carriers = scan_intersection_carriers(&stream());
        let carrier = carriers.get(&9).expect("composite decoded");
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) = &carrier.carrier.geometry else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.degree, 1);
        assert_eq!(curve.control_points.len(), 3);
        assert_eq!(curve.control_points[1], Point3::new(10.0, 0.0, 0.0));
        assert_eq!(curve.knots.len(), 5);
        assert!((curve.knots[2] - 0.01).abs() < 1e-12);
        assert!((curve.knots[3] - 0.02).abs() < 1e-12);
        let parameterization = carrier
            .parameterization
            .as_ref()
            .expect("width-four UV lanes decoded");
        assert_eq!(parameterization.supports, [2, 3]);
        let PcurveGeometry::Nurbs {
            control_points,
            knots,
            ..
        } = &parameterization.pcurves[0]
        else {
            panic!("expected a degree-one UV cache");
        };
        assert_eq!(
            control_points,
            &[
                Point2::new(0.0, 1.0),
                Point2::new(4.0, 5.0),
                Point2::new(8.0, 9.0)
            ]
        );
        assert_eq!(knots, &curve.knots);
        assert_eq!(parameterization.parameter_range, [0.0, 0.02]);
        assert_eq!(parameterization.model_points, curve.control_points);
        assert_eq!(parameterization.fit_tolerance_mm, 0.01);
    }

    #[test]
    fn intersection_data_entity_uses_the_same_composite_payload() {
        let mut bytes = intersection_data(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));

        let carrier = scan_intersection_carriers(&bytes)
            .remove(&9)
            .expect("intersection-data entity decoded");
        assert_eq!(carrier.carrier.offset, 0);
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) = carrier.carrier.geometry else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.control_points.len(), POINTS.len());
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

        let carriers = scan_intersection_carriers(&bytes);
        let carrier = carriers.get(&9).expect("decreasing chart decoded");
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) = &carrier.carrier.geometry else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.knots, [-0.02, -0.02, -0.01, 0.0, 0.0]);
        assert_eq!(
            curve.control_points,
            [
                Point3::new(10.0, 10.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ]
        );
        let parameterization = carrier.parameterization.as_ref().expect("UV cache");
        let PcurveGeometry::Nurbs { control_points, .. } = &parameterization.pcurves[0] else {
            panic!("expected a UV NURBS cache");
        };
        assert_eq!(control_points[0], Point2::new(8.0, 9.0));
        assert_eq!(control_points[2], Point2::new(0.0, 1.0));
        assert_eq!(parameterization.parameter_range, [-0.02, 0.0]);
    }

    #[test]
    fn seam_row_uv_count_is_accepted() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len() + 1));
        let carriers = scan_intersection_carriers(&bytes);
        let carrier = carriers.get(&9).expect("seam-row carrier decoded");
        assert!(carrier.parameterization.is_none());
    }

    #[test]
    fn terminator_outside_chordal_error_is_rejected() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, [0.011, 0.01, 0.0]));
        bytes.extend(uv(7, POINTS.len()));
        assert!(scan_intersection_carriers(&bytes).is_empty());
    }

    #[test]
    fn terminator_within_chordal_error_replaces_endpoint() {
        let end = [0.010_000_002, 0.01, 0.0];
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, end));
        bytes.extend(uv(7, POINTS.len()));
        let carriers = scan_intersection_carriers(&bytes);
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) = &carriers
            .get(&9)
            .expect("composite decoded")
            .carrier
            .geometry
        else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(
            *curve.control_points.last().expect("points"),
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
        assert!(scan_intersection_carriers(&bytes).is_empty());
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
        let carriers = scan_intersection_carriers(&bytes);
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) = &carriers
            .get(&9)
            .expect("ring composite decoded")
            .carrier
            .geometry
        else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.control_points.len(), 4);
        assert_eq!(curve.control_points[0], curve.control_points[3]);
    }

    #[test]
    fn mismatched_uv_count_is_rejected() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len() + 2));
        assert!(scan_intersection_carriers(&bytes).is_empty());
    }

    #[test]
    fn extended_chart_stride_is_selected_by_witnesses() {
        let chart_bytes = extended_chart(4, &POINTS, [0.5, 0.0, 0.0]);
        let (_, candidates) = chart_candidates(&chart_bytes, 2).expect("chart candidates");
        assert_eq!(candidates.len(), 2);

        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart_bytes);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));

        let carriers = scan_intersection_carriers(&bytes);
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) = &carriers
            .get(&9)
            .expect("extended chart decoded")
            .carrier
            .geometry
        else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.control_points.len(), POINTS.len());
        assert_eq!(
            *curve.control_points.last().expect("points"),
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
        let (_, candidates) = chart_candidates(&chart_bytes, 2).expect("chart candidates");
        assert_eq!(candidates.len(), 2);

        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart_bytes);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, end));
        bytes.extend(uv(7, 2));
        assert!(scan_intersection_carriers(&bytes).is_empty());
    }
}
