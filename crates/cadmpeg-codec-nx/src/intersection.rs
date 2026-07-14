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

/// A complete type-59 second-support bridge record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlendBound {
    /// Cross-reference index of the bridge record.
    pub xmt: u32,
    /// Five ordered common-header references.
    pub header_references: [u32; 5],
    /// Serialized orientation sense.
    pub sense: bool,
    /// Zero- or one-valued blend boundary index.
    pub boundary_index: u32,
    /// Cross-reference index of the blend surface.
    pub blend_surface: u32,
    /// Whether the record tag uses the `0xff` envelope escape.
    pub escaped: bool,
    /// Type-tag offset in the inflated stream.
    pub pos: usize,
}

/// Serialized framing of one `term_use` endpoint record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermUseFraming {
    /// Direct `0x0029` tag.
    Direct,
    /// `0x0029ff` escaped tag.
    Escaped,
    /// Payload following the inline `term_use` descriptor.
    DescriptorInline,
}

/// A complete `term_use` endpoint record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TermUse {
    /// Cross-reference index of the endpoint record.
    pub xmt: u32,
    /// Serialized leading count.
    pub count: u32,
    /// Two-byte endpoint-form discriminator.
    pub form: [u8; 2],
    /// Endpoint position in millimetres.
    pub point: Point3,
    /// Serialized record framing.
    pub framing: TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub pos: usize,
}

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
    /// Ordered support UV values in native Parasolid parameter units.
    pub support_uv: SupportUv,
    /// Two ext11 UV lanes awaiting assignment to the ordered supports.
    pub ext_support_uv: SupportUv,
}

/// Rejection census for structurally decoded intersection constructions whose
/// solved chart carrier is incomplete or inconsistent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RejectionCounts {
    /// The construction's `CHART_s` reference did not resolve to a valid chart.
    pub missing_chart: usize,
    /// The start term-use reference did not resolve.
    pub missing_start_term: usize,
    /// The end term-use reference did not resolve.
    pub missing_end_term: usize,
    /// A term-use endpoint lies outside the chart's chordal-error contract.
    pub endpoint_mismatch: usize,
}

impl RejectionCounts {
    /// Total rejected construction count.
    pub fn total(self) -> usize {
        self.missing_chart
            + self.missing_start_term
            + self.missing_end_term
            + self.endpoint_mismatch
    }

    fn add(&mut self, rejection: Rejection) {
        match rejection {
            Rejection::MissingChart => self.missing_chart += 1,
            Rejection::MissingStartTerm => self.missing_start_term += 1,
            Rejection::MissingEndTerm => self.missing_end_term += 1,
            Rejection::EndpointMismatch => self.endpoint_mismatch += 1,
        }
    }

    /// Add another stream's rejection census.
    pub fn extend(&mut self, other: Self) {
        self.missing_chart += other.missing_chart;
        self.missing_start_term += other.missing_start_term;
        self.missing_end_term += other.missing_end_term;
        self.endpoint_mismatch += other.endpoint_mismatch;
    }
}

/// Complete chart-carrier scan result.
#[derive(Debug, Clone, Default)]
pub struct CurveScan {
    /// Structurally valid constructions with a solved chart or a typed inbound
    /// curve reference.
    pub constructions: Vec<CompositeCurve>,
    /// Constructions with a complete solved 3D chart carrier.
    pub curves: Vec<IntersectionCurve>,
    /// Exact rejection census for the remaining parsed constructions.
    pub rejected: RejectionCounts,
}

#[derive(Debug, Clone, Copy)]
enum Rejection {
    MissingChart,
    MissingStartTerm,
    MissingEndTerm,
    EndpointMismatch,
}

#[derive(Debug, Clone)]
struct Chart {
    points: Vec<Point3>,
    parameters: Vec<f64>,
    fit_tolerance: f64,
    ext_support_uv: SupportUv,
}

#[derive(Debug, Clone)]
struct ChartPoints {
    points: Vec<Point3>,
    native_parameters: Option<Vec<f64>>,
    ext_support_uv: SupportUv,
}

/// Decode type-38 and single-byte `0x5a` records whose referenced chart and
/// endpoint witnesses form a complete solved cache.
pub fn curves(stream: &[u8]) -> Vec<IntersectionCurve> {
    scan(stream).curves
}

/// Decode chart-backed constructions and classify every rejected construction.
pub fn scan(stream: &[u8]) -> CurveScan {
    let charts = chart_records(stream);
    let terms = term_records(stream);
    let uv = uv_records(stream);
    let bridges = blend_bound_records(stream);
    let graph = topology::Graph::parse(stream);
    let referenced_curves = graph.referenced_curve_xmts();
    let mut result = CurveScan::default();
    for construction in topology::composite_curves(stream)
        .into_iter()
        .chain(topology::intersection_data_curves(stream))
    {
        match enrich(construction, &charts, &terms, &uv, &bridges, &graph) {
            Ok(curve) => {
                result.constructions.push(construction);
                result.curves.push(curve);
            }
            Err(rejection) if referenced_curves.contains(&construction.xmt) => {
                result.constructions.push(construction);
                result.rejected.add(rejection);
            }
            Err(_) => {}
        }
    }
    result
}

fn enrich(
    construction: CompositeCurve,
    charts: &BTreeMap<u32, Chart>,
    terms: &BTreeMap<u32, Point3>,
    uv: &BTreeMap<u32, SupportUv>,
    bridges: &BTreeMap<u32, u32>,
    graph: &topology::Graph,
) -> Result<IntersectionCurve, Rejection> {
    let start = terms
        .get(&construction.references[3])
        .ok_or(Rejection::MissingStartTerm)?;
    let end = terms
        .get(&construction.references[4])
        .ok_or(Rejection::MissingEndTerm)?;
    let chart = charts
        .get(&construction.references[2])
        .ok_or(Rejection::MissingChart)?;
    if distance(
        *start,
        *chart.points.first().ok_or(Rejection::MissingChart)?,
    ) > chart.fit_tolerance
        || distance(*end, *chart.points.last().ok_or(Rejection::MissingChart)?)
            > chart.fit_tolerance
    {
        return Err(Rejection::EndpointMismatch);
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
    Ok(IntersectionCurve {
        xmt: construction.xmt,
        references: construction.references,
        supports: [primary, secondary],
        pos: construction.pos,
        points: chart.points.clone(),
        parameters: chart.parameters.clone(),
        fit_tolerance: chart.fit_tolerance,
        support_uv,
        ext_support_uv: chart.ext_support_uv.clone(),
    })
}

fn blend_bound_records(stream: &[u8]) -> BTreeMap<u32, u32> {
    blend_bounds(stream)
        .into_iter()
        .map(|bound| (bound.xmt, bound.blend_surface))
        .collect()
}

/// Decode complete type-59 second-support bridge records.
pub fn blend_bounds(stream: &[u8]) -> Vec<BlendBound> {
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
            if !valid || header[0] != 1 {
                continue;
            }
            let sense = match stream.get(at) {
                Some(b'+') => true,
                Some(b'-') => false,
                _ => continue,
            };
            at += 1;
            let Some((boundary, consumed)) = read_xmt(stream, at) else {
                continue;
            };
            let Some((surface, _)) = read_xmt(stream, at + consumed) else {
                continue;
            };
            if boundary <= 1 && surface > 1 {
                out.entry(xmt).or_insert(BlendBound {
                    xmt,
                    header_references: header,
                    sense,
                    boundary_index: boundary,
                    blend_surface: surface,
                    escaped: escape == 1,
                    pos: tag,
                });
                break;
            }
        }
    }
    out.into_values().collect()
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
                ext_support_uv: chart_points.ext_support_uv,
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
                    entry.get_mut().ext_support_uv = candidate.ext_support_uv;
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
            let parameter_lanes = [
                [be::f64_at(stream, at + 24)?, be::f64_at(stream, at + 40)?],
                [be::f64_at(stream, at + 32)?, be::f64_at(stream, at + 48)?],
            ];
            ((norm - 1.0).abs() < 1.0e-9 && parameter.is_finite()).then_some((
                point,
                parameter,
                parameter_lanes,
            ))
        })
        .collect::<Option<Vec<_>>>();
    if let Some(entries) = ext {
        let mut points = Vec::with_capacity(entries.len());
        let mut native_parameters = Vec::with_capacity(entries.len());
        let mut ext_support_uv = [Some(Vec::new()), Some(Vec::new())];
        for (point, parameter, lanes) in entries {
            points.push(point);
            native_parameters.push(parameter);
            for lane in 0..2 {
                if lanes[lane]
                    .iter()
                    .all(|value| value.is_finite() && *value != MISSING_PARAMETER)
                {
                    if let Some(values) = &mut ext_support_uv[lane] {
                        values.push(lanes[lane]);
                    }
                } else {
                    ext_support_uv[lane] = None;
                }
            }
        }
        if native_parameters.windows(2).all(|pair| pair[0] < pair[1]) {
            return Some(ChartPoints {
                points,
                native_parameters: Some(native_parameters),
                ext_support_uv,
            });
        }
    }
    let points = (0..count)
        .map(|index| point_m(stream, block + index * 24))
        .collect::<Option<Vec<_>>>()?;
    (points.windows(2).any(|pair| pair[0] != pair[1])).then_some(ChartPoints {
        points,
        native_parameters: None,
        ext_support_uv: [None, None],
    })
}

fn term_records(stream: &[u8]) -> BTreeMap<u32, Point3> {
    term_use_records(stream)
        .into_iter()
        .map(|term| (term.xmt, term.point))
        .collect()
}

/// Decode complete direct, escaped, and descriptor-inline `term_use` records.
pub fn term_use_records(stream: &[u8]) -> Vec<TermUse> {
    let mut out = BTreeMap::new();
    for tag in find_tags(stream, [0, 41]) {
        for escape in [0usize, 1] {
            if escape == 1 && stream.get(tag + 2) != Some(&0xff) {
                continue;
            }
            let base = tag + 2 + escape;
            let framing = if escape == 0 {
                TermUseFraming::Direct
            } else {
                TermUseFraming::Escaped
            };
            if let Some(term) = term_at(stream, base, framing, tag) {
                out.entry(term.xmt).or_insert(term);
                break;
            }
        }
    }
    for label in find_bytes(stream, b"term_use") {
        let tail = label + b"term_use".len();
        if stream.get(tail..tail + INLINE_TERM_TAIL.len()) == Some(INLINE_TERM_TAIL) {
            let pos = tail + INLINE_TERM_TAIL.len();
            if let Some(term) = term_at(stream, pos, TermUseFraming::DescriptorInline, pos) {
                out.entry(term.xmt).or_insert(term);
            }
        }
    }
    out.into_values().collect()
}

fn term_at(stream: &[u8], base: usize, framing: TermUseFraming, pos: usize) -> Option<TermUse> {
    let count = be::u32_at(stream, base)?;
    let (xmt, xmt_len) = read_xmt(stream, base + 4)?;
    let payload = base + 4 + xmt_len;
    let form: [u8; 2] = stream.get(payload..payload + 2)?.try_into().ok()?;
    let valid = (count == 1 && form == *b"L?") || (count == 2 && matches!(&form, b"TF" | b"TS"));
    valid.then_some(())?;
    Some(TermUse {
        xmt,
        count,
        form,
        point: point_m(stream, payload + 2)?,
        framing,
        pos,
    })
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
