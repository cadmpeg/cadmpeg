// SPDX-License-Identifier: Apache-2.0
//! Surface-intersection curve carriers.
//!
//! An INTERSECTION node (`00 26`) carries a curve defined by the intersection
//! of two support surfaces. Its payload points to a `00 28` chart record (the
//! solved point cache) and two `00 29` limit records (the exact curve
//! endpoints). When the embedded schema appends `intersection_data`, a sixth
//! pointer names a `00 cc` support-UV record. Every index and pointer is an XT
//! positive-integer cell. Referenced limits select the chart entry width and
//! replace its approximate endpoints. A complete width-4 UV record
//! additionally yields co-parameterized support pcurve caches.

use std::collections::HashMap;

use cadmpeg_core::bytes::find_iter;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::loss::LossNote;

use super::typed::schema;
use super::{CurveCarrier, LEN_TO_MM};

use crate::layout::chart_00_28 as chart_layout;
use crate::layout::support_uv_00_cc as support_uv;
use crate::parasolid::xt_integer;

/// The XT null real, which marks an absent chart value.
const MISSING_PARAMETER: f64 = -31_415_800_000_000.0;
/// Fixed bytes between an inline `term_use` label and its terminator body.
const INLINE_TERM_TAIL: &[u8] = b"\x00\x00\x00\x01\x01\x63\x43\x5a";
/// Fixed bytes between an inline `values` label and its support-UV body.
const INLINE_UV_TAIL: &[u8] = b"\x00\x00\x00\x02\x01\x66\x01";
const INTERSECTION_TAG: [u8; 2] = [0x00, 0x26];
/// Pointers between the node id and the sense byte.
const COMMON_POINTERS: usize = 5;
/// Base-schema payload pointers: surface0, surface1, chart, start, end.
const BASE_PAYLOAD_POINTERS: usize = 5;
/// The appended field that adds the support-UV payload pointer.
const INTERSECTION_DATA_FIELD: &str = "intersection_data";
const NULL_POINTER: u32 = 1;

/// One decoded chart: solved points in metres and parameter bookkeeping.
struct Chart {
    endpoints: [[f64; 3]; 2],
    interior_points: Vec<[f64; 3]>,
    base_parameter: f64,
    base_scale: f64,
    /// `None` for the null value.
    chordal_error: Option<f64>,
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
    /// `None` when the chart's chordal error is null.
    pub(super) fit_tolerance_mm: Option<f64>,
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
    fit_tolerance_mm: Option<f64>,
    reversed: bool,
    endpoint_displacement: f64,
}

/// The INTERSECTION field list of one stream.
struct NodeSchema {
    payload_pointers: usize,
    /// Tag offset and field offset of the edited first-of-type node.
    first: Option<(usize, usize)>,
}

/// One framed INTERSECTION node.
struct Node {
    attr: u16,
    supports: [u16; 2],
    chart: u32,
    start: u32,
    end: u32,
    intersection_data: Option<u32>,
    /// Offset of the byte after the last payload pointer.
    end_offset: usize,
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

/// The stream's INTERSECTION schema. A stream whose edited program changes a
/// base field, appends any field other than `intersection_data`, or states a
/// second program has no known node layout.
fn node_schema(bytes: &[u8]) -> Option<NodeSchema> {
    let mut first = None;
    for at in find_iter(bytes, &INTERSECTION_TAG) {
        let Some(program) = schema::edited_program(bytes, at) else {
            continue;
        };
        let payload_pointers = match (program.base_copied, program.appended.as_slice()) {
            (true, []) => BASE_PAYLOAD_POINTERS,
            (true, [INTERSECTION_DATA_FIELD]) => BASE_PAYLOAD_POINTERS + 1,
            _ => return None,
        };
        if first
            .replace((payload_pointers, at, program.payload))
            .is_some()
        {
            return None;
        }
    }
    Some(match first {
        Some((payload_pointers, tag, fields)) => NodeSchema {
            payload_pointers,
            first: Some((tag, fields)),
        },
        None => NodeSchema {
            payload_pointers: BASE_PAYLOAD_POINTERS,
            first: None,
        },
    })
}

/// Tag offset and field offset of every INTERSECTION node.
fn intersection_nodes<'a>(
    bytes: &'a [u8],
    node_schema: &NodeSchema,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    let first = node_schema.first;
    find_iter(bytes, &INTERSECTION_TAG)
        .filter(move |tag| first.is_none_or(|(program, _)| program != *tag))
        .map(move |tag| {
            let fields = tag + INTERSECTION_TAG.len();
            if bytes.get(fields) == Some(&0xff) {
                (tag, fields + 1)
            } else {
                (tag, fields)
            }
        })
        .chain(first)
}

/// Frame one node: `attr node_id:u32 refs[5] sense(0x2b|0x2d)` and the
/// payload pointers. The next node starts with a `00` byte.
fn node_at(bytes: &[u8], fields: usize, payload_pointers: usize) -> Option<Node> {
    let mut at = fields;
    let attr = xt_integer::read_key(bytes, &mut at)?;
    at = at.checked_add(4)?;
    for _ in 0..COMMON_POINTERS {
        xt_integer::read_code(bytes, &mut at)?;
    }
    if !matches!(bytes.get(at), Some(0x2b | 0x2d)) {
        return None;
    }
    at += 1;
    let mut payload = [NULL_POINTER; BASE_PAYLOAD_POINTERS + 1];
    for pointer in payload.iter_mut().take(payload_pointers) {
        *pointer = xt_integer::read_code(bytes, &mut at)?;
    }
    if bytes.get(at).is_some_and(|next| *next != 0) {
        return None;
    }
    let [surface0, surface1, chart, start, end, intersection_data] = payload;
    Some(Node {
        attr,
        supports: [u16::try_from(surface0).ok()?, u16::try_from(surface1).ok()?],
        chart,
        start,
        end,
        intersection_data: (intersection_data != NULL_POINTER).then_some(intersection_data),
        end_offset: at,
    })
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

/// Parse every `00 28` chart record, keyed by its index cell.
fn chart_records(bytes: &[u8]) -> HashMap<u32, Vec<Chart>> {
    let mut out: HashMap<u32, Vec<Chart>> = HashMap::new();
    for body in record_bodies(bytes, 0x28) {
        let Some((attr, candidates)) = chart_candidates(bytes, body) else {
            continue;
        };
        out.entry(attr).or_default().extend(candidates);
    }
    out
}

/// A chart body is `count:u32 attr`, then the [`chart_layout`] fields and
/// `count` point entries (88-byte entries carrying a finite nonzero tangent at
/// +56, or bare 24-byte points).
fn chart_candidates(bytes: &[u8], body: usize) -> Option<(u32, Vec<Chart>)> {
    let count = View::u32_be_at(bytes, body).and_then(|count| usize::try_from(count).ok())?;
    let mut preamble = body.checked_add(4)?;
    let attr = xt_integer::read_code(bytes, &mut preamble)?;
    let (Some(base_parameter), Some(base_scale), Some(chart_count), Some(chordal_error)) = (
        View::f64_be_at(bytes, preamble + chart_layout::BASE_PARAMETER),
        View::f64_be_at(bytes, preamble + chart_layout::BASE_SCALE),
        View::u32_be_at(bytes, preamble + chart_layout::CHART_COUNT)
            .and_then(|count| usize::try_from(count).ok()),
        View::f64_be_at(bytes, preamble + chart_layout::CHORDAL_ERROR),
    ) else {
        return None;
    };
    let chordal_error = (chordal_error != MISSING_PARAMETER).then_some(chordal_error);
    if !(2..=4096).contains(&count)
        || chart_count != count
        || !base_parameter.is_finite()
        || !base_scale.is_finite()
        || base_scale == 0.0
        || chordal_error.is_some_and(|error| !error.is_finite() || error < 0.0)
        || View::f64_be_at(bytes, preamble + chart_layout::ABSENT_SENTINEL_A)
            != Some(MISSING_PARAMETER)
        || View::f64_be_at(bytes, preamble + chart_layout::ABSENT_SENTINEL_B)
            != Some(MISSING_PARAMETER)
    {
        return None;
    }
    let block = preamble + chart_layout::LEN;
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
        let mut interior_points = Vec::new();
        for index in 1..count - 1 {
            if let Some(point) = finite_point(bytes, block + index * stride) {
                interior_points.push(point);
            }
        }
        if !extended && first == last && interior_points.iter().all(|point| *point == first) {
            continue;
        }
        candidates.push(Chart {
            endpoints: [first, last],
            interior_points,
            base_parameter,
            base_scale,
            chordal_error,
        });
    }
    (!candidates.is_empty()).then_some((attr, candidates))
}

/// Parse a terminator body: `count:u32 attr`, a kind label, then the
/// endpoint. The label is one kind character (`L` limit, `H` ring, `T`
/// terminator, `B` spine boundary) with an optional second character (`?`,
/// `F`, or `S`). Both label widths yield a candidate endpoint; composite
/// validation selects the candidate that matches the chart.
fn term_at(bytes: &[u8], body: usize, out: &mut HashMap<u32, Vec<[f64; 3]>>) {
    let Some(count) = View::u32_be_at(bytes, body) else {
        return;
    };
    let mut label = body + 4;
    let Some(attr) = xt_integer::read_code(bytes, &mut label) else {
        return;
    };
    if !(1..=2).contains(&count) {
        return;
    }
    if !matches!(bytes.get(label), Some(b'L' | b'H' | b'T' | b'B')) {
        return;
    }
    let two_char = matches!(bytes.get(label + 1), Some(b'?' | b'F' | b'S'));
    for label_len in [2usize, 1] {
        if label_len == 2 && !two_char {
            continue;
        }
        if let Some(point) = finite_point(bytes, label + label_len) {
            out.entry(attr).or_default().push(point);
        }
    }
}

/// Every `00 29` or inline `term_use` terminator, keyed by its index cell.
fn term_records(bytes: &[u8]) -> HashMap<u32, Vec<[f64; 3]>> {
    let mut out: HashMap<u32, Vec<[f64; 3]>> = HashMap::new();
    for body in record_bodies(bytes, 0x29) {
        term_at(bytes, body, &mut out);
    }
    for label in find_iter(bytes, b"term_use") {
        let tail = label + b"term_use".len();
        if bytes.get(tail..tail + INLINE_TERM_TAIL.len()) == Some(INLINE_TERM_TAIL) {
            term_at(bytes, tail + INLINE_TERM_TAIL.len(), &mut out);
        }
    }
    out
}

/// Parse a support-UV body: `count:u32 attr width_marker:u8(2|3|4)` then
/// `count` finite f64 values.
fn uv_at(bytes: &[u8], body: usize) -> Option<(u32, UvRecord)> {
    let count = View::u32_be_at(bytes, body + support_uv::COUNT)
        .and_then(|count| usize::try_from(count).ok())?;
    let mut marker = body.checked_add(support_uv::ATTR)?;
    let attr = xt_integer::read_code(bytes, &mut marker)?;
    let width = match bytes.get(marker)? {
        2 | 3 => UvWidth::Two,
        4 => UvWidth::Four,
        _ => return None,
    };
    if count < width.values_per_point() * 2 || !count.is_multiple_of(width.values_per_point()) {
        return None;
    }
    let values_at = marker.checked_add(1)?;
    if cadmpeg_core::decode::bounded_len(
        u64_from_index(count),
        8,
        bytes.len().checked_sub(values_at).map_or(0, |len| len),
    ) != Some(count)
    {
        return None;
    }
    let mut values = Vec::new();
    for index in 0..count {
        values
            .push(View::f64_be_at(bytes, values_at + index * 8).filter(|value| value.is_finite())?);
    }
    Some((attr, UvRecord { width, values }))
}

/// Every `00 cc` or inline `values` support-UV record, keyed by its index cell.
fn uv_records(bytes: &[u8]) -> HashMap<u32, Vec<UvRecord>> {
    let mut out: HashMap<u32, Vec<UvRecord>> = HashMap::new();
    for body in record_bodies(bytes, 0xcc) {
        if let Some((attr, shape)) = uv_at(bytes, body) {
            out.entry(attr).or_default().push(shape);
        }
    }
    for label in find_iter(bytes, b"values") {
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
    Point3::from(a).distance(Point3::from(b))
}

/// Build the derived polyline curve for one validated composite.
fn solved_curve(
    chart: &Chart,
    start: [f64; 3],
    end: [f64; 3],
    record_attr: u16,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<(CurveGeometry, Vec<f64>, bool)> {
    let mut parameter = chart.base_parameter;
    let mut parameters = Vec::new();
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
    let mut points = Vec::new();
    points.push(start);
    points.extend(chart.interior_points.iter().copied());
    points.push(end);
    let reversed = if parameters.windows(2).all(|pair| pair[0] < pair[1]) {
        false
    } else if parameters.windows(2).all(|pair| pair[0] > pair[1]) {
        parameters.reverse();
        points.reverse();
        true
    } else {
        return None;
    };
    let (first, last) = if reversed {
        (parameter, chart.base_parameter)
    } else {
        (chart.base_parameter, parameter)
    };
    let mut knots = Vec::new();
    knots.push(first);
    knots.extend(parameters.iter().copied());
    knots.push(last);
    let controls = points
        .iter()
        .map(|p| Point3::new(p[0] * LEN_TO_MM, p[1] * LEN_TO_MM, p[2] * LEN_TO_MM))
        .collect::<Vec<_>>();
    let nurbs = match cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        1, knots, controls, None, false,
    ) {
        Ok(nurbs) => nurbs,
        Err(error) => {
            refusal.note(
                format_args!("sldprt intersection chart curve for record attr {record_attr}"),
                &error,
            );
            return None;
        }
    };
    Some((
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
        parameters,
        reversed,
    ))
}

fn solved_support_uv(
    parameters: &[f64],
    reversed: bool,
    records: Option<&[UvRecord]>,
) -> Option<[Vec<Point2>; 2]> {
    let expected_values = parameters.len().checked_mul(4)?;
    let records = records?;
    let mut candidate: Option<[Vec<Point2>; 2]> = None;
    for record in records {
        if record.width != UvWidth::Four || record.values.len() != expected_values {
            continue;
        }
        let mut controls = [Vec::new(), Vec::new()];
        for (support, control_points) in controls.iter_mut().enumerate() {
            control_points.extend(
                record
                    .values
                    .chunks_exact(4)
                    .map(|row| Point2::new(row[support * 2], row[support * 2 + 1])),
            );
            if reversed {
                control_points.reverse();
            }
        }
        if candidate
            .as_ref()
            .is_some_and(|previous| previous != &controls)
        {
            return None;
        }
        candidate = Some(controls);
    }
    candidate
}

fn nearest_term(
    records: &HashMap<u32, Vec<[f64; 3]>>,
    attr: u32,
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
/// Terminators replace the approximate chart endpoints. When both 24-byte and
/// 88-byte chart strides frame, the referenced terminators select the unique
/// candidate with the least endpoint displacement. A pointer that names no
/// record in the stream leaves the curve unresolved. An absent or
/// inconsistent optional UV record does not invalidate the model-space curve;
/// only a unique complete width-4 record supplies solved pcurves.
pub(super) fn scan_intersection_carriers(
    bytes: &[u8],
    lane_refusals: &mut Vec<LossNote>,
) -> HashMap<u16, IntersectionCarrier> {
    let charts = chart_records(bytes);
    let terms = term_records(bytes);
    if charts.is_empty() || terms.is_empty() {
        return HashMap::new();
    }
    let Some(node_schema) = node_schema(bytes) else {
        return HashMap::new();
    };
    let uvs = uv_records(bytes);
    let mut out = HashMap::new();
    for (offset, fields) in intersection_nodes(bytes, &node_schema) {
        let Some(node) = node_at(bytes, fields, node_schema.payload_pointers) else {
            continue;
        };
        let attr = node.attr;
        let Some(candidates) = charts.get(&node.chart) else {
            continue;
        };
        let mut chart_refusal = crate::lane_refusal::LaneRefusals::new();
        let mut selected: Option<SolvedChart> = None;
        let mut ambiguous = false;
        for chart in candidates {
            let [first, last] = chart.endpoints;
            let Some((start, start_distance)) = nearest_term(&terms, node.start, first) else {
                continue;
            };
            let Some((end, end_distance)) = nearest_term(&terms, node.end, last) else {
                continue;
            };
            let endpoint_displacement = start_distance + end_distance;
            let Some((geometry, parameters, reversed)) =
                solved_curve(chart, start, end, attr, &mut chart_refusal)
            else {
                continue;
            };
            let fit_tolerance_mm = chart.chordal_error.map(|error| error * LEN_TO_MM);
            if fit_tolerance_mm.is_some_and(|tolerance| !tolerance.is_finite()) {
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
        for record in chart_refusal.take_records() {
            let note = crate::loss::spline_lane_refusal(format_args!(
                "intersection chart for attr {attr}: {record}"
            ));
            lane_refusals.push(note);
        }
        let Some(selected) = selected else {
            continue;
        };
        if ambiguous {
            continue;
        }
        let support_uv = solved_support_uv(
            &selected.parameters,
            selected.reversed,
            node.intersection_data
                .and_then(|data| uvs.get(&data))
                .map(Vec::as_slice),
        );
        out.entry(attr).or_insert(IntersectionCarrier {
            carrier: CurveCarrier {
                runs_against_edges: false,
                attr,
                offset,
                end: node.end_offset,
                geometry: selected.geometry,
                parameter_range: None,
            },
            support_data: IntersectionSupportData {
                supports: node.supports,
                fit_tolerance_mm: selected.fit_tolerance_mm,
                support_uv,
            },
        });
    }
    out
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
            chordal_error: Some(1e-210),
        };
        let (_, parameters, reversed) = super::solved_curve(
            &chart,
            endpoints[0],
            endpoints[1],
            0,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .unwrap();
        assert_eq!(parameters, vec![0., 1.]);
        assert!(!reversed);
    }

    use super::super::LEN_TO_MM;
    use super::{
        chart_candidates, chart_records, scan_intersection_carriers, uv_at, uv_records, UvWidth,
        MISSING_PARAMETER,
    };
    use crate::parasolid::xt_integer;
    use cadmpeg_core::convert::f64_from_index;
    use cadmpeg_ir::geometry::CurveGeometry;
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::math::Point3;

    const POINTS: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.01, 0.01, 0.0]];
    /// A code that needs the two-word cell `fe d0 00 01`.
    const WIDE: u32 = 33_071;

    fn cell(code: u32) -> Vec<u8> {
        let (words, count) = xt_integer::encode(code).expect("code has a cell");
        words[..count]
            .iter()
            .flat_map(|word| word.to_be_bytes())
            .collect()
    }

    fn chart_with_error(attr: u32, points: &[[f64; 3]], chordal_error: f64) -> Vec<u8> {
        let mut bytes = vec![0, 0x28];
        bytes.extend_from_slice(
            &u32::try_from(points.len())
                .expect("point count fits u32")
                .to_be_bytes(),
        );
        bytes.extend(cell(attr));
        bytes.extend_from_slice(&0.0f64.to_be_bytes());
        bytes.extend_from_slice(&1.0f64.to_be_bytes());
        bytes.extend_from_slice(
            &u32::try_from(points.len())
                .expect("point count fits u32")
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&chordal_error.to_be_bytes());
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

    fn chart(attr: u32, points: &[[f64; 3]]) -> Vec<u8> {
        chart_with_error(attr, points, 1e-5)
    }

    fn extended_chart(attr: u32, points: &[[f64; 3]], tangent: [f64; 3]) -> Vec<u8> {
        let mut bytes = vec![0, 0x28];
        bytes.extend_from_slice(
            &u32::try_from(points.len())
                .expect("point count fits u32")
                .to_be_bytes(),
        );
        bytes.extend(cell(attr));
        bytes.extend_from_slice(&0.0f64.to_be_bytes());
        bytes.extend_from_slice(&1.0f64.to_be_bytes());
        bytes.extend_from_slice(
            &u32::try_from(points.len())
                .expect("point count fits u32")
                .to_be_bytes(),
        );
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

    fn term_with_label(attr: u32, label: &[u8], point: [f64; 3]) -> Vec<u8> {
        let mut bytes = vec![0, 0x29];
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend(cell(attr));
        bytes.extend_from_slice(label);
        for value in point {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    fn term(attr: u32, point: [f64; 3]) -> Vec<u8> {
        term_with_label(attr, b"L?", point)
    }

    fn uv(attr: u32, rows: usize) -> Vec<u8> {
        let mut bytes = vec![0, 0xcc];
        bytes.extend_from_slice(
            &u32::try_from(rows * 4)
                .expect("row count fits u32")
                .to_be_bytes(),
        );
        bytes.extend(cell(attr));
        bytes.push(4);
        for index in 0..rows * 4 {
            bytes.extend_from_slice(
                &f64_from_index(index)
                    .expect("index is exactly representable")
                    .to_be_bytes(),
            );
        }
        bytes
    }

    /// INTERSECTION fields: `attr node_id refs[5] sense payload`.
    fn fields(attr: u32, common: [u32; 5], payload: &[u32]) -> Vec<u8> {
        let mut bytes = cell(attr);
        bytes.extend_from_slice(&[0u8; 4]);
        for reference in common {
            bytes.extend(cell(reference));
        }
        bytes.push(0x2b);
        for reference in payload {
            bytes.extend(cell(*reference));
        }
        bytes
    }

    /// A base-schema INTERSECTION node with five payload pointers.
    fn composite(attr: u32, payload: [u32; 5]) -> Vec<u8> {
        let mut bytes = vec![0, 0x26];
        bytes.extend(fields(attr, [1; 5], &payload));
        bytes
    }

    /// A later INTERSECTION node of a schema that appends `intersection_data`.
    fn data_composite(attr: u32, payload: [u32; 6]) -> Vec<u8> {
        let mut bytes = vec![0, 0x26];
        bytes.extend(fields(attr, [1; 5], &payload));
        bytes
    }

    /// The edited first-of-type INTERSECTION schema program `instructions`,
    /// whose appended field is `intersection_data`, then the first node.
    fn program_composite(instructions: &[u8], attr: u32, payload: [u32; 6]) -> Vec<u8> {
        let current = instructions
            .iter()
            .filter(|instruction| matches!(instruction, b'C' | b'I' | b'A'))
            .count();
        let mut bytes = vec![0, 0x26, u8::try_from(current).expect("field count fits u8")];
        for instruction in instructions {
            bytes.push(*instruction);
            if *instruction == b'A' {
                bytes.push(17);
                bytes.extend_from_slice(b"intersection_data");
                bytes.extend_from_slice(&0x00cc_u16.to_be_bytes());
                bytes.extend(cell(1));
            }
        }
        bytes.push(b'Z');
        bytes.extend(fields(attr, [1; 5], &payload));
        bytes
    }

    fn base_program() -> Vec<u8> {
        let mut instructions = vec![b'C'; 11];
        instructions.push(b'A');
        instructions
    }

    fn stream() -> Vec<u8> {
        let mut bytes = program_composite(&base_program(), 9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));
        bytes
    }

    #[test]
    fn parasolid_intersection_chart_builds_a_curve() {
        let chart = super::Chart {
            endpoints: [POINTS[0], POINTS[2]],
            interior_points: vec![POINTS[1]],
            base_parameter: 0.0,
            base_scale: 1.0,
            chordal_error: Some(0.00001),
        };
        assert!(super::solved_curve(
            &chart,
            POINTS[0],
            POINTS[2],
            9,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .is_some());
    }

    #[test]
    fn parasolid_intersection_support_uv_splits_width_four_rows() {
        let records = [super::UvRecord {
            width: UvWidth::Four,
            values: vec![0.0; 12],
        }];
        let parameters = [0.0, 0.5, 1.0];
        assert_eq!(
            super::solved_support_uv(&parameters, false, Some(&records))
                .map(|controls| [controls[0].len(), controls[1].len()]),
            Some([3, 3])
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
        let mut bytes = program_composite(&base_program(), 9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &points));
        bytes.extend(term(5, *points.first().unwrap()));
        bytes.extend(term(6, *points.last().unwrap()));
        bytes.extend(record);
        let charts = chart_records(&bytes);
        let records = uv_records(&bytes);
        let chart = &charts[&4][0];
        let uv = &records[&7][0];
        assert_eq!(chart.interior_points.len() + 2, 9);
        assert!(uv.width == UvWidth::Two);
        assert_eq!(uv.values.len(), (chart.interior_points.len() + 2) * 2);
        assert!(scan_intersection_carriers(&bytes, &mut Vec::new()).contains_key(&9));
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
        assert!(uv_at(&record, 2).is_some());
        let mut bytes = program_composite(&base_program(), 9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(record);
        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
        let carrier = carriers
            .get(&9)
            .expect("width-two support leaves the curve available");
        assert!(carrier.support_data.support_uv.is_none());
    }

    #[test]
    fn consistent_composite_yields_polyline() {
        let carriers = scan_intersection_carriers(&stream(), &mut Vec::new());
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
        assert_eq!(support_data.fit_tolerance_mm, Some(0.01));
    }

    #[test]
    fn edited_schema_nodes_carry_the_intersection_data_pointer() {
        let mut bytes = program_composite(&base_program(), 9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(data_composite(10, [2, 3, 4, 5, 6, 7]));
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));

        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
        let first = carriers.get(&9).expect("first-of-type node decoded");
        assert_eq!(first.carrier.offset, 0);
        let later = carriers.get(&10).expect("later node decoded");
        for carrier in [first, later] {
            let Some(SolvedCurveGeometry::Nurbs(curve)) = carrier.carrier.geometry.solved() else {
                panic!("expected a NURBS polyline");
            };
            assert_eq!(curve.control_points().len(), POINTS.len());
            assert!(carrier.support_data.support_uv.is_some());
        }
    }

    #[test]
    fn base_schema_node_has_no_support_uv_pointer() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        // The two bytes after the last pointer are the next node's `00 28` tag.
        bytes.extend(uv(0x0028, POINTS.len()));

        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
        let carrier = carriers.get(&9).expect("base-schema node decoded");
        assert_eq!(carrier.carrier.end, 2 + 2 + 4 + 5 * 2 + 1 + 5 * 2);
        assert!(carrier.support_data.support_uv.is_none());
    }

    #[test]
    fn four_byte_cells_move_the_following_fields() {
        let mut bytes = vec![0, 0x26];
        bytes.extend(fields(9, [WIDE, 1, 1, 1, 1], &[2, 3, WIDE, WIDE + 1, 6]));
        bytes.extend(chart(WIDE, &POINTS));
        bytes.extend(term(WIDE + 1, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));

        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
        let carrier = carriers.get(&9).expect("wide-cell node decoded");
        assert_eq!(carrier.support_data.supports, [2, 3]);
        let Some(SolvedCurveGeometry::Nurbs(curve)) = carrier.carrier.geometry.solved() else {
            panic!("expected a NURBS polyline");
        };
        assert_eq!(curve.control_points().len(), POINTS.len());
    }

    #[test]
    fn zero_chordal_error_is_an_exact_chart() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6]);
        bytes.extend(chart_with_error(4, &POINTS, 0.0));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
        let carrier = carriers.get(&9).expect("zero-error chart decoded");
        assert_eq!(carrier.support_data.fit_tolerance_mm, Some(0.0));
    }

    #[test]
    fn null_chordal_error_gives_no_fit_tolerance() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6]);
        bytes.extend(chart_with_error(4, &POINTS, MISSING_PARAMETER));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
        let carrier = carriers.get(&9).expect("null-error chart decoded");
        assert_eq!(carrier.support_data.fit_tolerance_mm, None);
    }

    #[test]
    fn spine_boundary_limit_is_a_curve_end() {
        let mut bytes = composite(9, [2, 3, 4, 5, 6]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term_with_label(6, b"B", POINTS[2]));
        assert!(scan_intersection_carriers(&bytes, &mut Vec::new()).contains_key(&9));
    }

    #[test]
    fn edited_program_that_changes_a_base_field_has_no_node_layout() {
        let mut instructions = vec![b'C'; 10];
        instructions.extend_from_slice(b"DA");
        let mut bytes = program_composite(&instructions, 9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));
        assert!(scan_intersection_carriers(&bytes, &mut Vec::new()).is_empty());
    }

    #[test]
    fn negative_chart_scale_reverses_curve_and_uv_caches_atomically() {
        let mut bytes = program_composite(&base_program(), 9, [2, 3, 4, 5, 6, 7]);
        let mut chart = chart(4, &POINTS);
        chart[16..24].copy_from_slice(&(-1.0f64).to_be_bytes());
        bytes.extend(chart);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len()));

        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
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
        let mut bytes = program_composite(&base_program(), 9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len() + 1));
        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
        let carrier = carriers.get(&9).expect("seam-row carrier decoded");
        assert!(carrier.support_data.support_uv.is_none());
    }

    #[test]
    fn exact_terminator_replaces_an_approximate_chart_endpoint() {
        let end = [0.011, 0.01, 0.0];
        let mut bytes = composite(9, [2, 3, 4, 5, 6]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, end));
        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
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
        let mut bytes = composite(9, [2, 3, 4, 5, 6]);
        let mut bad = chart(4, &POINTS);
        let at = bad.len() - POINTS.len() * 24 - 16;
        bad[at..at + 8].copy_from_slice(&0.0f64.to_be_bytes());
        bytes.extend(bad);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        assert!(scan_intersection_carriers(&bytes, &mut Vec::new()).is_empty());
    }

    #[test]
    fn ring_composite_with_one_char_label_and_no_uv_record_decodes() {
        let ring = [
            [0.0, 0.0, 0.0],
            [0.01, 0.0, 0.0],
            [0.01, 0.01, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let mut bytes = composite(9, [2, 3, 4, 5, 5]);
        bytes.extend(chart(4, &ring));
        bytes.extend(term_with_label(5, b"H", ring[0]));
        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
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
        let mut bytes = program_composite(&base_program(), 9, [2, 3, 4, 5, 6, 7]);
        bytes.extend(chart(4, &POINTS));
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));
        bytes.extend(uv(7, POINTS.len() + 2));
        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
        assert!(carriers.contains_key(&9));
        assert!(carriers[&9].support_data.support_uv.is_none());
    }

    #[test]
    fn extended_chart_stride_is_selected_by_witnesses() {
        let chart_bytes = extended_chart(4, &POINTS, [0.5, 0.0, 0.0]);
        let (_, candidates) = chart_candidates(&chart_bytes, 2).expect("chart candidates");
        assert_eq!(candidates.len(), 2);

        let mut bytes = composite(9, [2, 3, 4, 5, 6]);
        bytes.extend(chart_bytes);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, POINTS[2]));

        let carriers = scan_intersection_carriers(&bytes, &mut Vec::new());
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
        let (_, candidates) = chart_candidates(&chart_bytes, 2).expect("chart candidates");
        assert_eq!(candidates.len(), 2);

        let mut bytes = composite(9, [2, 3, 4, 5, 6]);
        bytes.extend(chart_bytes);
        bytes.extend(term(5, POINTS[0]));
        bytes.extend(term(6, end));
        assert!(scan_intersection_carriers(&bytes, &mut Vec::new()).is_empty());
    }
}
