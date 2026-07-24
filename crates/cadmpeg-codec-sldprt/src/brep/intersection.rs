// SPDX-License-Identifier: Apache-2.0
//! Surface-intersection curve carriers.
//!
//! A `00 26` composite record or `00 01 5a` intersection-data entity carries a
//! curve defined by the intersection of two support surfaces. Its payload
//! references a `00 28` chart record (the solved point cache), two `00 29`
//! terminator records (the exact curve endpoints), and a `00 cc` support-UV
//! record. A carrier whose referenced chart, terminators, and UV values are
//! mutually consistent yields a derived degree-one NURBS curve through the
//! chart points with the terminators as endpoints.

use std::collections::HashMap;

use cadmpeg_ir::be::{f64_at, u16_at, u32_at};
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve};
use cadmpeg_ir::math::Point3;

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

fn unit_tangent(bytes: &[u8], at: usize) -> bool {
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
    let norm = tangent.iter().map(|value| value * value).sum::<f64>();
    (norm - 1.0).abs() < 1e-9
}

/// Parse every `00 28` chart record: `count:u32 attr:u16 base_parameter:f64
/// base_scale:f64 chart_count:u32 chordal_error:f64`, two [`MISSING_PARAMETER`]
/// sentinels at +36/+44, then `count` point entries at +52 (88-byte entries
/// carrying a unit tangent at +56, or bare 24-byte points).
fn chart_records(bytes: &[u8]) -> HashMap<u16, Vec<Chart>> {
    let mut out: HashMap<u16, Vec<Chart>> = HashMap::new();
    for body in record_bodies(bytes, 0x28) {
        let Some(chart) = chart_at(bytes, body) else {
            continue;
        };
        out.entry(chart.0).or_default().push(chart.1);
    }
    out
}

fn chart_at(bytes: &[u8], body: usize) -> Option<(u16, Chart)> {
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
    let extended = block + 88 * count <= bytes.len()
        && (0..count).all(|index| unit_tangent(bytes, block + index * 88 + 56));
    let stride = if extended { 88 } else { 24 };
    if block + stride * count > bytes.len() {
        return None;
    }
    let points = (0..count)
        .map(|index| finite_point(bytes, block + index * stride))
        .collect::<Option<Vec<_>>>()?;
    if !extended && points.windows(2).all(|pair| pair[0] == pair[1]) {
        return None;
    }
    Some((
        attr,
        Chart {
            points,
            base_parameter,
            base_scale,
            chordal_error,
        },
    ))
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
/// `count` finite f64 values. Only the value count participates in composite
/// validation.
fn uv_at(bytes: &[u8], body: usize) -> Option<(u16, (usize, usize))> {
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
    for index in 0..count {
        if !f64_at(bytes, body + 7 + index * 8)?.is_finite() {
            return None;
        }
    }
    Some((attr, (width, count)))
}

/// Every `00 cc` or inline `values` support-UV record, keyed by attribute,
/// as `(width, value_count)`.
fn uv_records(bytes: &[u8]) -> HashMap<u16, Vec<(usize, usize)>> {
    let mut out: HashMap<u16, Vec<(usize, usize)>> = HashMap::new();
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

/// Build the derived polyline curve for one validated composite.
fn solved_curve(chart: &Chart, start: [f64; 3], end: [f64; 3]) -> CurveGeometry {
    let mut points = chart.points.clone();
    *points.first_mut().expect("chart has at least two points") = start;
    *points.last_mut().expect("chart has at least two points") = end;
    let mut parameters = Vec::with_capacity(points.len());
    parameters.push(chart.base_parameter);
    for pair in points.windows(2) {
        let previous = *parameters.last().expect("base parameter inserted");
        parameters.push(previous + distance(pair[0], pair[1]) * chart.base_scale);
    }
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.push(parameters[0]);
    knots.extend_from_slice(&parameters);
    knots.push(*parameters.last().expect("non-empty parameters"));
    CurveGeometry::Nurbs(NurbsCurve {
        degree: 1,
        knots,
        control_points: points
            .iter()
            .map(|p| Point3::new(p[0] * LEN_TO_MM, p[1] * LEN_TO_MM, p[2] * LEN_TO_MM))
            .collect(),
        weights: None,
        periodic: false,
    })
}

/// Scan intersection carriers whose chart, terminator, and UV witnesses are
/// mutually consistent, keyed by carrier attribute.
///
/// The composite body is `attr:u16 ordinal:u32 refs:u16[5] marker:u8(0x2b|0x2d)`
/// then six payload references `[support0, support1, chart, term_start,
/// term_end, uv]`. Both terminators must sit within the chart chordal error of
/// the corresponding chart endpoint (a ring names one terminator twice), and a
/// resolvable UV record must cover the chart points (with an optional extra
/// row at a periodic seam).
pub(super) fn scan_intersection_carriers(bytes: &[u8]) -> HashMap<u16, Carrier> {
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
        let geometry = candidates.iter().find_map(|chart| {
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
            uvs.get(&uv_ref)
                .is_none_or(|shapes| {
                    shapes
                        .iter()
                        .any(|(width, count)| *count == width * n || *count == width * (n + 1))
                })
                .then(|| solved_curve(chart, *start, *end))
        });
        if let Some(geometry) = geometry {
            out.entry(attr).or_insert(Carrier {
                attr,
                offset,
                end: payload + 12,
                geometry: CarrierGeometry::Curve(geometry),
                frame: None,
                orientation_reversed: false,
            });
        }
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
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) = &carrier.geometry else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.degree, 1);
        assert_eq!(curve.control_points.len(), 3);
        assert_eq!(curve.control_points[1], Point3::new(10.0, 0.0, 0.0));
        assert_eq!(curve.knots.len(), 5);
        assert!((curve.knots[2] - 0.01).abs() < 1e-12);
        assert!((curve.knots[3] - 0.02).abs() < 1e-12);
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
        assert_eq!(carrier.offset, 0);
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) = carrier.geometry else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.control_points.len(), POINTS.len());
    }

    #[test]
    fn seam_row_uv_count_is_accepted() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len() + 1));
        assert!(scan_intersection_carriers(&bytes).contains_key(&9));
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
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) =
            &carriers.get(&9).expect("composite decoded").geometry
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
        let CarrierGeometry::Curve(CurveGeometry::Nurbs(curve)) =
            &carriers.get(&9).expect("ring composite decoded").geometry
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
}
