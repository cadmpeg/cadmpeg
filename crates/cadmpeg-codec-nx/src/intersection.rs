// SPDX-License-Identifier: Apache-2.0
//! Decode chart-backed Parasolid surface-intersection constructions.

use std::collections::BTreeMap;

use cadmpeg_ir::be;
use cadmpeg_ir::math::Point3;

use crate::topology::{self, CompositeCurve};

const MISSING_PARAMETER: f64 = -31_415_800_000_000.0;
const INLINE_TERM_TAIL: &[u8] = b"\x00\x00\x00\x01\x01\x63\x43\x5a";
const INLINE_UV_TAIL: &[u8] = b"\x00\x00\x00\x02\x01\x66\x01";
type SupportUv = [Option<Vec<[f64; 2]>>; 2];

/// A decoded surface-intersection construction and its solved chart cache.
#[derive(Debug, Clone)]
pub struct IntersectionCurve {
    /// Cross-reference index of the construction record.
    pub xmt: u32,
    /// Six ordered construction references.
    pub references: [u32; 6],
    /// Resolved primary and secondary support-surface references.
    pub supports: [u32; 2],
    /// Type-tag offset of the construction record.
    pub pos: usize,
    /// Chart points in millimetres.
    pub points: Vec<Point3>,
    /// Native chart parameter at each point.
    pub parameters: Vec<f64>,
    /// Chart chordal error in millimetres.
    pub fit_tolerance: f64,
    /// Ordered support-zero UV values in native Parasolid parameter units.
    pub support_uv: SupportUv,
}

#[derive(Debug, Clone)]
struct Chart {
    points: Vec<Point3>,
    parameters: Vec<f64>,
    fit_tolerance: f64,
}

#[derive(Debug, Clone)]
struct ChartPoints {
    points: Vec<Point3>,
    native_parameters: Option<Vec<f64>>,
}

/// Decode type-38 and single-byte `0x5a` records whose referenced chart and
/// endpoint witnesses form a complete solved cache.
pub fn curves(stream: &[u8]) -> Vec<IntersectionCurve> {
    let charts = chart_records(stream);
    let terms = term_records(stream);
    let uv = uv_records(stream);
    let bridges = blend_bound_records(stream);
    let graph = topology::Graph::parse(stream);
    topology::composite_curves(stream)
        .into_iter()
        .chain(topology::intersection_data_curves(stream))
        .filter_map(|construction| enrich(construction, &charts, &terms, &uv, &bridges, &graph))
        .collect()
}

fn enrich(
    construction: CompositeCurve,
    charts: &BTreeMap<u32, Chart>,
    terms: &BTreeMap<u32, Point3>,
    uv: &BTreeMap<u32, SupportUv>,
    bridges: &BTreeMap<u32, u32>,
    graph: &topology::Graph,
) -> Option<IntersectionCurve> {
    let chart = charts.get(&construction.references[2])?;
    let start = terms.get(&construction.references[3])?;
    let end = terms.get(&construction.references[4])?;
    if distance(*start, *chart.points.first()?) > chart.fit_tolerance
        || distance(*end, *chart.points.last()?) > chart.fit_tolerance
    {
        return None;
    }
    let support_uv = uv
        .get(&construction.references[5])
        .cloned()
        .unwrap_or([None, None]);
    let first_is_surface = is_surface(graph, construction.references[0]);
    let second_is_surface = is_surface(graph, construction.references[1]);
    let (primary, bridge) = if first_is_surface {
        (construction.references[0], construction.references[1])
    } else if second_is_surface {
        (construction.references[1], construction.references[0])
    } else {
        (1, 1)
    };
    let secondary = bridges
        .get(&bridge)
        .copied()
        .or_else(|| is_surface(graph, bridge).then_some(bridge))
        .filter(|secondary| *secondary != primary)
        .unwrap_or(1);
    Some(IntersectionCurve {
        xmt: construction.xmt,
        references: construction.references,
        supports: [primary, secondary],
        pos: construction.pos,
        points: chart.points.clone(),
        parameters: chart.parameters.clone(),
        fit_tolerance: chart.fit_tolerance,
        support_uv,
    })
}

fn blend_bound_records(stream: &[u8]) -> BTreeMap<u32, u32> {
    let mut out = BTreeMap::new();
    for tag in find_tags(stream, [0, 59]) {
        for escape in [0usize, 1] {
            if escape == 1 && stream.get(tag + 2) != Some(&0xff) {
                continue;
            }
            let mut at = tag + 2 + escape;
            let Some((xmt, consumed)) = read_xmt(stream, at) else {
                continue;
            };
            at += consumed + 4;
            let mut header = [0u32; 5];
            let mut valid = true;
            for reference in &mut header {
                let Some((value, consumed)) = read_xmt(stream, at) else {
                    valid = false;
                    break;
                };
                *reference = value;
                at += consumed;
            }
            if !valid || header[0] != 1 || !matches!(stream.get(at), Some(b'+' | b'-')) {
                continue;
            }
            at += 1;
            let Some((boundary, consumed)) = read_xmt(stream, at) else {
                continue;
            };
            let Some((surface, _)) = read_xmt(stream, at + consumed) else {
                continue;
            };
            if boundary <= 1 && surface > 1 {
                out.entry(xmt).or_insert(surface);
                break;
            }
        }
    }
    out
}

fn is_surface(graph: &topology::Graph, xmt: u32) -> bool {
    [50, 51, 52, 53, 54, 56, 60, 124]
        .into_iter()
        .any(|kind| graph.get(kind, xmt).is_some())
}

fn chart_records(stream: &[u8]) -> BTreeMap<u32, Chart> {
    let mut out = BTreeMap::new();
    for tag in find_tags(stream, [0, 40]) {
        for escape in [0usize, 1] {
            if escape == 1 && stream.get(tag + 2) != Some(&0xff) {
                continue;
            }
            let base = tag + 2 + escape;
            let Some(count) = be::u32_at(stream, base).map(|value| value as usize) else {
                continue;
            };
            if !(2..=1024).contains(&count) {
                continue;
            }
            let Some((xmt, xmt_len)) = read_xmt(stream, base + 4) else {
                continue;
            };
            let preamble = base + 4 + xmt_len;
            let Some(base_parameter) = be::f64_at(stream, preamble) else {
                continue;
            };
            let Some(base_scale) = be::f64_at(stream, preamble + 8) else {
                continue;
            };
            let Some(chart_count) = be::u32_at(stream, preamble + 16) else {
                continue;
            };
            let Some(chordal_error) = be::f64_at(stream, preamble + 20) else {
                continue;
            };
            let errors = [
                be::f64_at(stream, preamble + 36),
                be::f64_at(stream, preamble + 44),
            ];
            if chart_count as usize != count
                || !base_parameter.is_finite()
                || !base_scale.is_finite()
                || base_scale == 0.0
                || !chordal_error.is_finite()
                || chordal_error <= 0.0
                || errors != [Some(MISSING_PARAMETER), Some(MISSING_PARAMETER)]
            {
                continue;
            }
            let block = preamble + 52;
            let Some(chart_points) = chart_points(stream, block, count) else {
                continue;
            };
            let mut chord_parameters = Vec::with_capacity(chart_points.points.len());
            chord_parameters.push(base_parameter);
            for pair in chart_points.points.windows(2) {
                let chord_m = distance(pair[0], pair[1]) / 1000.0;
                chord_parameters.push(
                    chord_parameters
                        .last()
                        .copied()
                        .expect("invariant: base parameter inserted")
                        + chord_m * base_scale,
                );
            }
            let native_parameters = chart_points.native_parameters;
            let candidate = Chart {
                points: chart_points.points,
                parameters: native_parameters.clone().unwrap_or(chord_parameters),
                fit_tolerance: chordal_error * 1000.0,
            };
            match out.entry(xmt) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
                std::collections::btree_map::Entry::Occupied(mut entry)
                    if native_parameters.is_some()
                        && entry.get().points.len() == candidate.points.len()
                        && entry.get().points.iter().zip(&candidate.points).all(
                            |(first, second)| {
                                distance(*first, *second)
                                    <= entry.get().fit_tolerance.max(candidate.fit_tolerance)
                            },
                        ) =>
                {
                    entry.get_mut().parameters = candidate.parameters;
                }
                std::collections::btree_map::Entry::Occupied(_) => {}
            }
            break;
        }
    }
    out
}

fn chart_points(stream: &[u8], block: usize, count: usize) -> Option<ChartPoints> {
    let ext = (0..count)
        .map(|index| {
            let at = block + index * 88;
            let point = point_m(stream, at)?;
            let tangent = [
                be::f64_at(stream, at + 56)?,
                be::f64_at(stream, at + 64)?,
                be::f64_at(stream, at + 72)?,
            ];
            let norm = tangent.iter().map(|v| v * v).sum::<f64>().sqrt();
            let parameter = be::f64_at(stream, at + 80)?;
            ((norm - 1.0).abs() < 1.0e-9 && parameter.is_finite()).then_some((point, parameter))
        })
        .collect::<Option<Vec<_>>>();
    if let Some(entries) = ext {
        let (points, native_parameters): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        if native_parameters.windows(2).all(|pair| pair[0] < pair[1]) {
            return Some(ChartPoints {
                points,
                native_parameters: Some(native_parameters),
            });
        }
    }
    let points = (0..count)
        .map(|index| point_m(stream, block + index * 24))
        .collect::<Option<Vec<_>>>()?;
    (points.windows(2).any(|pair| pair[0] != pair[1])).then_some(ChartPoints {
        points,
        native_parameters: None,
    })
}

fn term_records(stream: &[u8]) -> BTreeMap<u32, Point3> {
    let mut out = BTreeMap::new();
    for tag in find_tags(stream, [0, 41]) {
        for escape in [0usize, 1] {
            if escape == 1 && stream.get(tag + 2) != Some(&0xff) {
                continue;
            }
            let base = tag + 2 + escape;
            if let Some((xmt, point)) = term_at(stream, base) {
                out.entry(xmt).or_insert(point);
                break;
            }
        }
    }
    for label in find_bytes(stream, b"term_use") {
        let tail = label + b"term_use".len();
        if stream.get(tail..tail + INLINE_TERM_TAIL.len()) == Some(INLINE_TERM_TAIL) {
            if let Some((xmt, point)) = term_at(stream, tail + INLINE_TERM_TAIL.len()) {
                out.entry(xmt).or_insert(point);
            }
        }
    }
    out
}

fn term_at(stream: &[u8], base: usize) -> Option<(u32, Point3)> {
    let count = be::u32_at(stream, base)?;
    let (xmt, xmt_len) = read_xmt(stream, base + 4)?;
    let payload = base + 4 + xmt_len;
    let valid = (count == 1 && stream.get(payload..payload + 2) == Some(b"L?"))
        || (count == 2 && matches!(stream.get(payload..payload + 2), Some(b"TF" | b"TS")));
    valid
        .then(|| point_m(stream, payload + 2))
        .flatten()
        .map(|point| (xmt, point))
}

fn uv_records(stream: &[u8]) -> BTreeMap<u32, SupportUv> {
    let mut out = BTreeMap::new();
    for tag in find_tags(stream, [0, 204]) {
        for escape in [0usize, 1] {
            if escape == 1 && stream.get(tag + 2) != Some(&0xff) {
                continue;
            }
            let base = tag + 2 + escape;
            if let Some((xmt, values)) = uv_at(stream, base) {
                out.entry(xmt).or_insert(values);
                break;
            }
        }
    }
    for label in find_bytes(stream, b"values") {
        let tail = label + b"values".len();
        if stream.get(tail..tail + INLINE_UV_TAIL.len()) == Some(INLINE_UV_TAIL) {
            if let Some((xmt, values)) = uv_at(stream, tail + INLINE_UV_TAIL.len()) {
                out.entry(xmt).or_insert(values);
            }
        }
    }
    out
}

fn uv_at(stream: &[u8], base: usize) -> Option<(u32, SupportUv)> {
    let count = be::u32_at(stream, base)? as usize;
    let (xmt, xmt_len) = read_xmt(stream, base + 4)?;
    let payload = base + 4 + xmt_len;
    let marker @ 2..=4 = stream.get(payload).copied()? else {
        return None;
    };
    let width = if marker == 4 { 4 } else { 2 };
    if count < width * 2 || !count.is_multiple_of(width) {
        return None;
    }
    let values = (0..count)
        .map(|index| be::f64_at(stream, payload + 1 + index * 8))
        .collect::<Option<Vec<_>>>()?;
    if !values.iter().all(|value| value.is_finite()) {
        return None;
    }
    let first = values
        .chunks_exact(width)
        .map(|entry| [entry[0], entry[1]])
        .collect();
    let second = (marker == 4).then(|| {
        values
            .chunks_exact(4)
            .map(|entry| [entry[2], entry[3]])
            .collect()
    });
    Some((xmt, [Some(first), second]))
}

fn find_tags(stream: &[u8], tag: [u8; 2]) -> impl Iterator<Item = usize> + '_ {
    stream
        .windows(2)
        .enumerate()
        .filter_map(move |(offset, bytes)| (bytes == tag).then_some(offset))
}

fn find_bytes<'a>(stream: &'a [u8], needle: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    stream
        .windows(needle.len())
        .enumerate()
        .filter_map(move |(offset, bytes)| (bytes == needle).then_some(offset))
}

fn point_m(stream: &[u8], at: usize) -> Option<Point3> {
    let xyz = be::vec3_at(stream, at)?;
    xyz.iter()
        .all(|value| value.is_finite() && value.abs() < 100.0)
        .then_some(Point3::new(
            xyz[0] * 1000.0,
            xyz[1] * 1000.0,
            xyz[2] * 1000.0,
        ))
}

fn distance(first: Point3, second: Point3) -> f64 {
    ((first.x - second.x).powi(2) + (first.y - second.y).powi(2) + (first.z - second.z).powi(2))
        .sqrt()
}

fn read_xmt(stream: &[u8], at: usize) -> Option<(u32, usize)> {
    let first = i16::from_be_bytes([*stream.get(at)?, *stream.get(at + 1)?]);
    if first >= 0 {
        return Some((first as u32, 2));
    }
    let remainder = first.unsigned_abs();
    let quotient = u16::from_be_bytes([*stream.get(at + 2)?, *stream.get(at + 3)?]);
    Some((u32::from(quotient) * 32_767 + u32::from(remainder), 4))
}
