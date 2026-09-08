// SPDX-License-Identifier: Apache-2.0
//! Decode bounded Parasolid surface-intersection constructions.

pub(crate) mod finite_point;
use finite_point::FinitePoint;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::bytes::find_iter;
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point3;
use serde::{Deserialize, Serialize};

pub(crate) mod blend_bound_state;
use blend_bound_state::BlendBoundState;
pub(crate) mod chart_samples;
pub(crate) mod support_uv_values;
use support_uv_values::{SupportUvPacking, SupportUvValues};

use chart_samples::{ChartPreamble, ChartSamples, SourceChartData, MISSING_PARAMETER};

use crate::framing::node_kind::NodeKind;
use crate::framing::read_xmt_width as read_xmt;
use crate::framing::xmt_reference::{NonNullXmt, XmtTarget};
use crate::layout::chart_s_preamble as chart_preamble;
use crate::topology::{self, CompositeCurve};

const EPS_INTERSECTION_CHART_POINTS_E9: f64 = 1.0e-9;

const INLINE_TERM_TAIL: &[u8] = b"\x00\x00\x00\x01\x01\x63\x43\x5a";
const INLINE_UV_TAIL: &[u8] = b"\x00\x00\x00\x02\x01\x66\x01";
/// Two ordered optional support-surface parameter lanes.
pub type SupportUv = [Option<SupportUvLane>; 2];

/// Support parameters checked against their chart sample count.
#[derive(Debug, Clone, PartialEq)]
pub struct SupportUvLane(Vec<[f64; 2]>);

impl SupportUvLane {
    /// Construct one parameter pair per chart sample.
    pub fn new(values: Vec<[f64; 2]>, sample_count: usize) -> Option<Self> {
        (values.len() == sample_count).then_some(Self(values))
    }

    /// Ordered support parameter pairs.
    pub fn as_slice(&self) -> &[[f64; 2]] {
        &self.0
    }
}

impl std::ops::Deref for SupportUvLane {
    type Target = [[f64; 2]];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

/// Serialized framing of one `CHART_s` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartFraming {
    /// Direct `0x0028` tag.
    Direct,
    /// `0x0028ff` escaped tag.
    Escaped,
}

/// Serialized Hvec layout of one `CHART_s` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartPointLayout {
    /// Three model-space coordinates per point.
    Xyz3,
    /// Eleven scalars containing point, two UV lanes, tangent, and parameter.
    Ext11,
}

/// Serialized framing of one type-59 blend-bound record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendBoundFraming {
    /// Partition-style fields following a direct `0x003b` tag.
    PartitionDirect,
    /// Partition-style fields following an escaped `0x003bff` tag.
    PartitionEscaped,
    /// Status-framed deltas fields following a direct `0x003b` tag.
    DeltasDirect,
    /// Status-framed deltas fields following an escaped `0x003bff` tag.
    DeltasEscaped,
}

/// One complete physical `CHART_s` source record.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSourceRecord {
    /// Cross-reference index of the chart.
    pub xmt: u32,
    /// Checked chart preamble.
    pub preamble: ChartPreamble,
    /// Points with exactly the fields admitted by their Hvec layout.
    pub data: SourceChartData,
    /// Serialized record framing.
    pub framing: ChartFraming,
    /// Type-tag offset in the inflated stream.
    pub pos: usize,
}

/// A complete type-59 second-support bridge record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlendBound {
    pub state: BlendBoundState,
    /// Serialized partition/deltas and direct/escaped framing.
    pub framing: BlendBoundFraming,
    /// Type-tag offset in the inflated stream.
    pub pos: usize,
}

/// Serialized framing of one `term_use` endpoint record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TermUseFraming {
    /// Direct `0x0029` tag.
    Direct,
    /// `0x0029ff` escaped tag.
    Escaped,
    /// Payload following the inline `term_use` descriptor.
    DescriptorInline,
}

/// Admitted endpoint-form encodings and their required leading counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TermUseForm {
    #[serde(rename = "L?")]
    LQuestion,
    #[serde(rename = "TF")]
    Tf,
    #[serde(rename = "TS")]
    Ts,
}

impl TermUseForm {
    pub fn count(self) -> u32 {
        match self {
            Self::LQuestion => 1,
            Self::Tf | Self::Ts => 2,
        }
    }
}

/// A complete `term_use` endpoint record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TermUse {
    /// Cross-reference index of the endpoint record.
    pub xmt: u32,
    /// Endpoint form, including its required leading count.
    pub form: TermUseForm,
    /// Endpoint position in millimetres.
    pub point: FinitePoint,
    /// Serialized record framing.
    pub framing: TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub pos: usize,
}

/// Serialized framing of one support-UV values array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportUvFraming {
    /// Direct `0x00cc` tag.
    Direct,
    /// `0x00ccff` escaped tag.
    Escaped,
    /// Payload following the inline `values` descriptor.
    DescriptorInline,
}

/// A complete support-UV values-array record.
#[derive(Debug, Clone, PartialEq)]
pub struct SupportUvRecord {
    /// Cross-reference index of the values array.
    pub xmt: u32,
    /// Exact finite packed support tuples.
    pub values: SupportUvValues,
    /// Serialized record framing.
    pub framing: SupportUvFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub pos: usize,
}

/// A decoded surface-intersection construction and its solved chart cache.
#[derive(Debug, Clone)]
pub struct IntersectionCurve {
    /// Six ordered construction references.
    pub references: [Option<XmtTarget>; 6],
    /// Cross-reference index of the construction record.
    pub xmt: u32,
    /// Resolved primary support-surface reference.
    pub primary_support: NonNullXmt,
    /// Resolved secondary support-surface reference.
    pub secondary_support: Option<NonNullXmt>,
    /// Type-tag offset of the construction record.
    pub pos: usize,
    /// Paired chart points in millimetres and native parameters.
    pub samples: ChartSamples,
    /// Chart chordal error in millimetres.
    pub fit_tolerance: f64,
    /// Ordered support UV values in native Parasolid parameter units.
    pub support_uv: SupportUv,
    /// Two ext11 UV lanes awaiting assignment to the ordered supports.
    pub ext_support_uv: SupportUv,
}

/// Two distinct non-null support-surface references.
#[derive(Debug, Clone, Copy)]
pub struct DistinctSupports([NonNullXmt; 2]);

impl DistinctSupports {
    fn new(first: NonNullXmt, second: NonNullXmt) -> Option<Self> {
        (first != second).then_some(Self([first, second]))
    }

    pub(crate) fn references(self) -> [NonNullXmt; 2] {
        self.0
    }
}

/// A bounded intersection relation without a solved chart cache.
#[derive(Debug, Clone, Copy)]
pub struct UnchartedIntersection {
    /// Cross-reference index of the construction record.
    pub xmt: u32,
    /// Two exact, distinct support-surface references.
    pub supports: DistinctSupports,
    /// Ordered endpoints of the unique topology edge in millimetres.
    pub endpoints: [Point3; 2],
    /// Edge tolerance in Parasolid metres.
    pub tolerance: f64,
}

/// Rejection census for structurally decoded intersection constructions whose
/// solved chart carrier is incomplete or inconsistent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RejectionCounts {
    /// The construction did not resolve a valid primary support relation.
    pub missing_support: usize,
    /// Two construction forms used one stream-local XMT identity.
    pub duplicate_identity: usize,
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
        self.missing_support
            + self.duplicate_identity
            + self.missing_chart
            + self.missing_start_term
            + self.missing_end_term
            + self.endpoint_mismatch
    }

    fn add(&mut self, rejection: Rejection) {
        match rejection {
            Rejection::MissingSupport => self.missing_support += 1,
            Rejection::DuplicateIdentity => self.duplicate_identity += 1,
            Rejection::MissingChart => self.missing_chart += 1,
            Rejection::MissingStartTerm => self.missing_start_term += 1,
            Rejection::MissingEndTerm => self.missing_end_term += 1,
            Rejection::EndpointMismatch => self.endpoint_mismatch += 1,
        }
    }

    /// Add another stream's rejection census.
    pub fn extend(&mut self, other: Self) {
        self.missing_support += other.missing_support;
        self.duplicate_identity += other.duplicate_identity;
        self.missing_chart += other.missing_chart;
        self.missing_start_term += other.missing_start_term;
        self.missing_end_term += other.missing_end_term;
        self.endpoint_mismatch += other.endpoint_mismatch;
    }
}

/// Complete chart-carrier scan result.
#[derive(Debug, Clone, Default)]
pub struct CurveScan {
    /// Every structurally valid construction found in the source graph before
    /// chart enrichment filters it. Native record extraction reuses this lane
    /// so it does not parse the same graph a second time.
    pub(crate) source_constructions: Vec<CompositeCurve>,
    /// Structurally valid constructions with a solved chart or a typed inbound
    /// curve reference.
    pub constructions: Vec<CompositeCurve>,
    /// Constructions with a complete solved 3D chart carrier.
    pub curves: Vec<IntersectionCurve>,
    /// Constructions bounded by exact topology witnesses but lacking a chart.
    pub uncharted: Vec<UnchartedIntersection>,
    /// Exact rejection census for the remaining parsed constructions.
    pub rejected: RejectionCounts,
}

#[derive(Debug, Clone, Copy)]
enum Rejection {
    MissingSupport,
    DuplicateIdentity,
    MissingChart,
    MissingStartTerm,
    MissingEndTerm,
    EndpointMismatch,
}

#[derive(Debug, Clone)]
struct Chart {
    samples: ChartSamples,
    fit_tolerance: f64,
    ext_support_uv: SupportUv,
}

/// Decode type-38 and single-byte `0x5a` records whose referenced chart and
/// endpoint witnesses form a complete solved cache.
pub fn curves(stream: &[u8], point_layout: ChartPointLayout) -> Vec<IntersectionCurve> {
    scan(stream, point_layout).curves
}

/// Decode chart-backed constructions and classify every rejected construction.
pub fn scan(stream: &[u8], point_layout: ChartPointLayout) -> CurveScan {
    let graph = topology::Graph::parse(stream);
    scan_with_graph(stream, &graph, point_layout)
}

pub(crate) fn scan_with_graph(
    stream: &[u8],
    graph: &topology::Graph,
    point_layout: ChartPointLayout,
) -> CurveScan {
    let uv = uv_records(stream);
    let constructions = graph
        .composite_curves()
        .into_iter()
        .chain(topology::intersection_data_curves(stream))
        .collect();
    scan_with_auxiliaries(
        &chart_records(stream, point_layout),
        &term_records(stream),
        &uv,
        &blend_bound_records(stream),
        graph,
        constructions,
        CrossFormCollision::Reject,
    )
}

/// Decode a merged partition/deltas stream with explicit auxiliary replacement boundaries.
#[cfg(test)]
pub(crate) fn scan_with_auxiliary_replacements(
    stream: &[u8],
    base_stream: &[u8],
    replacement_streams: &[&[u8]],
) -> CurveScan {
    let graph = topology::Graph::parse(stream);
    scan_with_auxiliary_replacements_and_graph(stream, base_stream, replacement_streams, &graph)
}

pub(crate) fn scan_with_auxiliary_replacements_and_graph(
    stream: &[u8],
    base_stream: &[u8],
    replacement_streams: &[&[u8]],
    graph: &topology::Graph,
) -> CurveScan {
    let mut charts = chart_records(base_stream, ChartPointLayout::Xyz3);
    let mut terms = term_records(base_stream);
    let mut uv = uv_records(base_stream);
    let mut bridges = blend_bound_records(base_stream);
    for replacement_stream in replacement_streams {
        charts.extend(chart_records(replacement_stream, ChartPointLayout::Ext11));
        terms.extend(term_records(replacement_stream));
        uv.extend(uv_records(replacement_stream));
        bridges.extend(blend_bound_records(replacement_stream));
    }
    let constructions = graph
        .composite_curves()
        .into_iter()
        .chain(topology::intersection_data_curves(stream))
        .collect();
    scan_with_auxiliaries(
        &charts,
        &terms,
        &uv,
        &bridges,
        graph,
        constructions,
        CrossFormCollision::PreferDeltaTwin,
    )
}

#[derive(Clone, Copy)]
enum CrossFormCollision {
    Reject,
    PreferDeltaTwin,
}

fn scan_with_auxiliaries(
    charts: &BTreeMap<u32, Chart>,
    terms: &BTreeMap<u32, Point3>,
    uv: &BTreeMap<u32, SupportUvValues>,
    bridges: &BTreeMap<u32, u32>,
    graph: &topology::Graph,
    constructions: Vec<CompositeCurve>,
    cross_form_collision: CrossFormCollision,
) -> CurveScan {
    let referenced_curves = graph.referenced_curve_xmts();
    let mut result = CurveScan::default();
    let mut forms_by_xmt = BTreeMap::<u32, BTreeSet<bool>>::new();
    for construction in &constructions {
        forms_by_xmt
            .entry(construction.xmt)
            .or_default()
            .insert(construction.delta_twin);
    }
    let cross_form_xmts = forms_by_xmt
        .into_iter()
        .filter_map(|(xmt, forms)| (forms.len() > 1).then_some(xmt))
        .collect::<BTreeSet<_>>();
    let constructions = constructions
        .into_iter()
        .filter(|construction| {
            if !cross_form_xmts.contains(&construction.xmt) {
                return true;
            }
            match cross_form_collision {
                CrossFormCollision::Reject => {
                    result.rejected.add(Rejection::DuplicateIdentity);
                    false
                }
                CrossFormCollision::PreferDeltaTwin => construction.delta_twin,
            }
        })
        .collect::<Vec<_>>();
    for construction in constructions.iter().copied() {
        match enrich(construction, charts, terms, uv, bridges, graph) {
            Ok(curve) => {
                result.constructions.push(construction);
                result.curves.push(curve);
            }
            Err(rejection)
                if referenced_curves.contains(&construction.xmt)
                    && construction_supports(construction, uv, bridges, graph).is_some()
                    && construction_has_endpoint_witnesses(construction, terms, graph) =>
            {
                result.constructions.push(construction);
                if matches!(rejection, Rejection::MissingChart) {
                    if let (Some(supports), Some(witness)) = (
                        construction_supports(construction, uv, bridges, graph).and_then(
                            |(primary, secondary)| DistinctSupports::new(primary, secondary?),
                        ),
                        graph
                            .unique_curve_edge_witness(construction.xmt)
                            .filter(|witness| {
                                witness.tolerance.is_finite()
                                    && witness.tolerance > 0.0
                                    && (witness.tolerance * 1000.0).is_finite()
                            }),
                    ) {
                        result.uncharted.push(UnchartedIntersection {
                            xmt: construction.xmt,
                            supports,
                            endpoints: witness.endpoints,
                            tolerance: witness.tolerance,
                        });
                    }
                }
                result.rejected.add(rejection);
            }
            Err(Rejection::MissingSupport) if referenced_curves.contains(&construction.xmt) => {
                result.rejected.add(Rejection::MissingSupport);
            }
            Err(_) => {}
        }
    }
    result.source_constructions = constructions;
    result
}

fn enrich(
    construction: CompositeCurve,
    charts: &BTreeMap<u32, Chart>,
    terms: &BTreeMap<u32, Point3>,
    uv: &BTreeMap<u32, SupportUvValues>,
    bridges: &BTreeMap<u32, u32>,
    graph: &topology::Graph,
) -> Result<IntersectionCurve, Rejection> {
    let chart = construction.references[2]
        .and_then(|target| charts.get(&u32::from(target)))
        .ok_or(Rejection::MissingChart)?;
    let chart_endpoints = chart.samples.endpoints();
    let serialized_terms = [
        construction.references[3]
            .and_then(|target| terms.get(&u32::from(target)))
            .copied(),
        construction.references[4]
            .and_then(|target| terms.get(&u32::from(target)))
            .copied(),
    ];
    if serialized_terms
        .iter()
        .zip(chart_endpoints)
        .any(|(term, endpoint)| {
            term.is_some_and(|term| distance(term, endpoint) > chart.fit_tolerance)
        })
    {
        return Err(Rejection::EndpointMismatch);
    }
    if serialized_terms.iter().any(Option::is_none) {
        let topology_endpoints = graph
            .unique_curve_edge_witness(construction.xmt)
            .map(|witness| witness.endpoints)
            .ok_or_else(|| {
                if serialized_terms[0].is_none() {
                    Rejection::MissingStartTerm
                } else {
                    Rejection::MissingEndTerm
                }
            })?;
        let matching_permutations = [[0usize, 1usize], [1usize, 0usize]]
            .into_iter()
            .filter(|permutation| {
                permutation.iter().enumerate().all(|(ordinal, topology)| {
                    distance(chart_endpoints[ordinal], topology_endpoints[*topology])
                        <= chart.fit_tolerance
                })
            })
            .count();
        if matching_permutations != 1 {
            return Err(if serialized_terms[0].is_none() {
                Rejection::MissingStartTerm
            } else {
                Rejection::MissingEndTerm
            });
        }
    }
    let (primary_support, secondary_support) =
        construction_supports(construction, uv, bridges, graph).ok_or(Rejection::MissingSupport)?;
    let support_uv = construction.references[5]
        .and_then(|target| uv.get(&u32::from(target)))
        .map_or([None, None], |values| {
            values.support_uv(chart.samples.points().len())
        });
    Ok(IntersectionCurve {
        references: construction.references,
        xmt: construction.xmt,
        primary_support,
        secondary_support,
        pos: construction.pos,
        samples: chart.samples.clone(),
        fit_tolerance: chart.fit_tolerance,
        support_uv,
        ext_support_uv: chart.ext_support_uv.clone(),
    })
}

fn construction_supports(
    construction: CompositeCurve,
    uv: &BTreeMap<u32, SupportUvValues>,
    bridges: &BTreeMap<u32, u32>,
    graph: &topology::Graph,
) -> Option<(NonNullXmt, Option<NonNullXmt>)> {
    let (primary, bridge) = if construction.delta_twin {
        (construction.references[0], construction.references[1])
    } else {
        // A present marker-3 values array explicitly reverses the serialized
        // support order. Without that array, retain the type-38 references'
        // order; no alternate order was serialized.
        match construction.references[5]
            .and_then(|target| uv.get(&u32::from(target)))
            .map(SupportUvValues::packing)
        {
            Some(SupportUvPacking::Form3) => {
                (construction.references[1], construction.references[0])
            }
            Some(SupportUvPacking::Form2 | SupportUvPacking::Form4) | None => {
                (construction.references[0], construction.references[1])
            }
        }
    };
    let primary = u32::from(primary?);
    is_surface(graph, primary).then_some(())?;
    let secondary = bridge
        .map(u32::from)
        .and_then(|bridge| {
            bridges
                .get(&bridge)
                .copied()
                .or_else(|| is_surface(graph, bridge).then_some(bridge))
        })
        .filter(|secondary| *secondary != primary)
        .and_then(|secondary| NonNullXmt::try_from(secondary).ok());
    Some((NonNullXmt::try_from(primary).ok()?, secondary))
}

fn construction_has_endpoint_witnesses(
    construction: CompositeCurve,
    terms: &BTreeMap<u32, Point3>,
    graph: &topology::Graph,
) -> bool {
    construction.references[2..=4].iter().all(Option::is_none)
        || construction.references[3..=4]
            .iter()
            .all(|reference| reference.is_some_and(|target| terms.contains_key(&u32::from(target))))
        || graph.unique_curve_edge_witness(construction.xmt).is_some()
}

fn blend_bound_records(stream: &[u8]) -> BTreeMap<u32, u32> {
    blend_bounds(stream)
        .into_iter()
        .map(|b| (b.state.xmt(), b.state.blend_surface()))
        .collect()
}

/// Decode complete type-59 second-support bridge records.
pub fn blend_bounds(stream: &[u8]) -> Vec<BlendBound> {
    let mut out = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    for tag in find_tags(stream, [0, 59]) {
        if let Some((bound, _)) = blend_bound_at(stream, tag) {
            insert_unique(&mut out, &mut duplicates, bound.state.xmt(), bound);
        }
    }
    out.into_values().collect()
}

pub(crate) fn blend_bound_at(stream: &[u8], tag: usize) -> Option<(BlendBound, usize)> {
    (stream.get(tag..tag + 2) == Some(&[0, 59])).then_some(())?;
    let mut candidate = None;
    for framing in [
        BlendBoundFraming::PartitionDirect,
        BlendBoundFraming::PartitionEscaped,
        BlendBoundFraming::DeltasDirect,
        BlendBoundFraming::DeltasEscaped,
    ] {
        if matches!(
            framing,
            BlendBoundFraming::PartitionEscaped | BlendBoundFraming::DeltasEscaped
        ) && stream.get(tag + 2) != Some(&0xff)
        {
            continue;
        }
        let Some((bound, end)) = blend_bound_layout(stream, tag, framing) else {
            continue;
        };
        if candidate.is_some() {
            return None;
        }
        candidate = Some((bound, end));
    }
    candidate
}

fn blend_bound_layout(
    stream: &[u8],
    tag: usize,
    framing: BlendBoundFraming,
) -> Option<(BlendBound, usize)> {
    let escaped = matches!(
        framing,
        BlendBoundFraming::PartitionEscaped | BlendBoundFraming::DeltasEscaped
    );
    let status_framed = matches!(
        framing,
        BlendBoundFraming::DeltasDirect | BlendBoundFraming::DeltasEscaped
    );
    let mut at = tag.checked_add(2 + usize::from(escaped))?;
    let (xmt, consumed) = read_xmt(stream, at)?;
    at = at.checked_add(consumed + 4)?;
    let mut header = [0u32; 5];
    for reference in &mut header {
        let (value, consumed) = read_xmt(stream, at)?;
        *reference = value;
        at += consumed;
        if status_framed {
            (*stream.get(at)? <= 1).then_some(())?;
            at += 1;
        }
    }
    let sense = match stream.get(at) {
        Some(b'+') => true,
        Some(b'-') => false,
        _ => return None,
    };
    at += 1;
    let (boundary, consumed) = read_xmt(stream, at)?;
    at += consumed;
    let (surface, consumed) = read_xmt(stream, at)?;
    at += consumed;
    if status_framed {
        (stream.get(at) == Some(&1)).then_some(())?;
        at += 1;
    }
    Some((
        BlendBound {
            state: BlendBoundState::new(xmt, header, sense, boundary, surface).ok()?,
            framing,
            pos: tag,
        },
        at,
    ))
}

fn is_surface(graph: &topology::Graph, xmt: u32) -> bool {
    [
        NodeKind::Plane,
        NodeKind::Cylinder,
        NodeKind::Cone,
        NodeKind::Sphere,
        NodeKind::Torus,
        NodeKind::BlendSurface,
        NodeKind::OffsetSurface,
        NodeKind::BSurface,
    ]
    .into_iter()
    .any(|kind| graph.get(kind, xmt).is_some())
}

fn chart_records(stream: &[u8], point_layout: ChartPointLayout) -> BTreeMap<u32, Chart> {
    let mut out = BTreeMap::new();
    let mut complemented = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for source in chart_source_records(stream, point_layout) {
        if duplicates.contains(&source.xmt) {
            continue;
        }
        let fit_tolerance = source.preamble.chordal_error() * 1000.0;
        if !fit_tolerance.is_finite() {
            continue;
        }
        let has_native_parameters = source.data.point_layout() == ChartPointLayout::Ext11;
        let Some((samples, ext_support_uv)) = source.data.into_samples(source.preamble) else {
            continue;
        };
        let candidate = Chart {
            samples,
            fit_tolerance,
            ext_support_uv,
        };
        match out.entry(source.xmt) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let complements = !complemented.contains(&source.xmt)
                    && has_native_parameters
                    && entry
                        .get()
                        .samples
                        .points()
                        .iter()
                        .zip(candidate.samples.points().iter())
                        .all(|(first, second)| {
                            distance(*first, *second)
                                <= entry.get().fit_tolerance.max(candidate.fit_tolerance)
                        })
                    && entry
                        .get_mut()
                        .samples
                        .replace_parameters_from(&candidate.samples);
                if complements {
                    entry.get_mut().ext_support_uv = candidate.ext_support_uv;
                    complemented.insert(source.xmt);
                } else {
                    entry.remove();
                    duplicates.insert(source.xmt);
                }
            }
        }
    }
    out
}

/// Decode every complete physical direct or escaped `CHART_s` source record.
pub fn chart_source_records(
    stream: &[u8],
    point_layout: ChartPointLayout,
) -> Vec<ChartSourceRecord> {
    let mut out = Vec::new();
    let mut tag = 0usize;
    while tag.saturating_add(2) <= stream.len() {
        if stream.get(tag..tag + 2) == Some(&[0, 40]) {
            if let Some((record, end)) = chart_source_record_at(stream, tag, point_layout) {
                out.push(record);
                // A complete chart owns its counted point lane. Do not rescan
                // bytes inside that lane as nested chart candidates.
                tag = end;
                continue;
            }
        }
        tag += 1;
    }
    out
}

pub(crate) fn chart_source_record_at(
    stream: &[u8],
    tag: usize,
    point_layout: ChartPointLayout,
) -> Option<(ChartSourceRecord, usize)> {
    (stream.get(tag..tag + 2) == Some(&[0, 40])).then_some(())?;
    for escape in [0usize, 1] {
        if escape == 1 && stream.get(tag + 2) != Some(&0xff) {
            continue;
        }
        let base = tag + 2 + escape;
        let Some(count) = View::over_retained(stream)
            .child(base, stream.len())
            .and_then(|mut view| view.u32_be())
            .and_then(|value| usize::try_from(value).ok())
        else {
            continue;
        };
        let Some((xmt, xmt_len)) = read_xmt(stream, base + 4) else {
            continue;
        };
        let preamble = base + 4 + xmt_len;
        let Some(mut head) = View::over_retained(stream).child(preamble, stream.len()) else {
            continue;
        };
        // Keep the sequential preamble unpack dense; rustfmt would undo the net deletion.
        #[rustfmt::skip]
        let (
            Some(base_parameter), Some(base_scale), Some(chart_count), Some(chordal_error),
            Some(angular_error), Some(e0), Some(e1),
        ) = (
            head.f64_be(), head.f64_be(), head.u32_be(), head.f64_be(), head.f64_be(),
            head.f64_be(), head.f64_be(),
        ) else { continue; };
        if chart_count as usize != count || [e0, e1] != [MISSING_PARAMETER, MISSING_PARAMETER] {
            continue;
        }
        let Ok(preamble_values) =
            ChartPreamble::new(base_parameter, base_scale, chordal_error, angular_error)
        else {
            continue;
        };
        let block = preamble + chart_preamble::LEN;
        let Some((data, end)) = chart_points(stream, block, count, point_layout) else {
            continue;
        };
        return Some((
            ChartSourceRecord {
                xmt,
                preamble: preamble_values,
                data,
                framing: if escape == 0 {
                    ChartFraming::Direct
                } else {
                    ChartFraming::Escaped
                },
                pos: tag,
            },
            end,
        ));
    }
    None
}

fn chart_points(
    stream: &[u8],
    block: usize,
    count: usize,
    point_layout: ChartPointLayout,
) -> Option<(SourceChartData, usize)> {
    let point_width = match point_layout {
        ChartPointLayout::Xyz3 => 24,
        ChartPointLayout::Ext11 => 88,
    };
    let end = block.checked_add(count.checked_mul(point_width)?)?;
    stream.get(block..end)?;
    if point_layout == ChartPointLayout::Xyz3 {
        let points = (0..count)
            .map(|index| point_m(stream, block + index * 24).map(Point3::from))
            .collect::<Option<Vec<_>>>()?;
        return Some((SourceChartData::xyz3(points).ok()?, end));
    }

    let mut points = Vec::with_capacity(count);
    let mut native_parameters = Vec::with_capacity(count);
    let mut ext_support_uv = [Some(Vec::new()), Some(Vec::new())];
    for index in 0..count {
        let (point, parameter, lanes) = chart_ext_point_at(stream, block + index * 88)?;
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
    Some((
        SourceChartData::ext11(points, native_parameters, ext_support_uv).ok()?,
        end,
    ))
}

fn chart_ext_point_at(stream: &[u8], at: usize) -> Option<(Point3, f64, [[f64; 2]; 2])> {
    let point = Point3::from(point_m(stream, at)?);
    let mut mid = View::over_retained(stream).child(at.checked_add(24)?, at.checked_add(88)?)?;
    let (u0, u1, v0, v1) = (mid.f64_be()?, mid.f64_be()?, mid.f64_be()?, mid.f64_be()?);
    let tangent = [mid.f64_be()?, mid.f64_be()?, mid.f64_be()?];
    let parameter = mid.f64_be()?;
    let norm = tangent.iter().map(|v| v * v).sum::<f64>().sqrt();
    let parameter_lanes = [[u0, v0], [u1, v1]];
    ((norm - 1.0).abs() < EPS_INTERSECTION_CHART_POINTS_E9 && parameter.is_finite()).then_some((
        point,
        parameter,
        parameter_lanes,
    ))
}

fn term_records(stream: &[u8]) -> BTreeMap<u32, Point3> {
    term_use_records(stream)
        .into_iter()
        .map(|term| (term.xmt, Point3::from(term.point)))
        .collect()
}

/// Decode complete direct, escaped, and descriptor-inline `term_use` records.
pub fn term_use_records(stream: &[u8]) -> Vec<TermUse> {
    let mut out = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    for tag in find_tags(stream, [0, 41]) {
        if let Some((term, _)) = term_use_at(stream, tag) {
            insert_unique(&mut out, &mut duplicates, term.xmt, term);
        }
    }
    for label in find_iter(stream, b"term_use") {
        let tail = label + b"term_use".len();
        if stream.get(tail..tail + INLINE_TERM_TAIL.len()) == Some(INLINE_TERM_TAIL) {
            let pos = tail + INLINE_TERM_TAIL.len();
            if let Some((term, _)) = term_at(stream, pos, TermUseFraming::DescriptorInline, pos) {
                insert_unique(&mut out, &mut duplicates, term.xmt, term);
            }
        }
    }
    out.into_values().collect()
}

pub(crate) fn term_use_at(stream: &[u8], tag: usize) -> Option<(TermUse, usize)> {
    (stream.get(tag..tag + 2) == Some(&[0, 41])).then_some(())?;
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
            return Some(term);
        }
    }
    None
}

fn term_at(
    stream: &[u8],
    base: usize,
    framing: TermUseFraming,
    pos: usize,
) -> Option<(TermUse, usize)> {
    let count = View::over_retained(stream)
        .child(base, stream.len())?
        .u32_be()?;
    let (xmt, xmt_len) = read_xmt(stream, base + 4)?;
    let payload = base + 4 + xmt_len;
    let form = match (count, stream.get(payload..payload + 2)?) {
        (1, b"L?") => TermUseForm::LQuestion,
        (2, b"TF") => TermUseForm::Tf,
        (2, b"TS") => TermUseForm::Ts,
        _ => return None,
    };
    Some((
        TermUse {
            xmt,
            form,
            point: point_m(stream, payload + 2)?,
            framing,
            pos,
        },
        payload.checked_add(26)?,
    ))
}

fn uv_records(stream: &[u8]) -> BTreeMap<u32, SupportUvValues> {
    support_uv_records(stream)
        .into_iter()
        .map(|record| (record.xmt, record.values))
        .collect()
}

/// Decode complete direct, escaped, and descriptor-inline support-UV arrays.
pub fn support_uv_records(stream: &[u8]) -> Vec<SupportUvRecord> {
    let mut out = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    let mut tag = 0usize;
    while tag.saturating_add(2) <= stream.len() {
        if stream.get(tag..tag + 2) == Some(&[0, 204]) {
            if let Some((record, end)) = support_uv_record_at(stream, tag) {
                insert_unique(&mut out, &mut duplicates, record.xmt, record);
                // A complete counted UV lane owns its scalar payload. Do not
                // rescan payload bytes as nested support arrays.
                tag = end;
                continue;
            }
        }
        tag += 1;
    }
    let mut label_start = 0;
    while label_start < stream.len() {
        let Some(relative) = find_iter(&stream[label_start..], b"values").next() else {
            break;
        };
        let label = label_start + relative;
        let tail = label + b"values".len();
        if stream.get(tail..tail + INLINE_UV_TAIL.len()) == Some(INLINE_UV_TAIL) {
            let pos = tail + INLINE_UV_TAIL.len();
            if let Some((record, end)) = uv_at(stream, pos, SupportUvFraming::DescriptorInline, pos)
            {
                insert_unique(&mut out, &mut duplicates, record.xmt, record);
                label_start = end;
                continue;
            }
        }
        label_start = label + 1;
    }
    out.into_values().collect()
}

pub(crate) fn support_uv_record_at(stream: &[u8], tag: usize) -> Option<(SupportUvRecord, usize)> {
    (stream.get(tag..tag + 2) == Some(&[0, 204])).then_some(())?;
    for escape in [0usize, 1] {
        if escape == 1 && stream.get(tag + 2) != Some(&0xff) {
            continue;
        }
        let base = tag + 2 + escape;
        let framing = if escape == 0 {
            SupportUvFraming::Direct
        } else {
            SupportUvFraming::Escaped
        };
        if let Some(record) = uv_at(stream, base, framing, tag) {
            return Some(record);
        }
    }
    None
}

fn insert_unique<T>(
    records: &mut BTreeMap<u32, T>,
    duplicates: &mut BTreeSet<u32>,
    xmt: u32,
    record: T,
) {
    if duplicates.contains(&xmt) {
        return;
    }
    if records.insert(xmt, record).is_some() {
        records.remove(&xmt);
        duplicates.insert(xmt);
    }
}

fn uv_at(
    stream: &[u8],
    base: usize,
    framing: SupportUvFraming,
    pos: usize,
) -> Option<(SupportUvRecord, usize)> {
    let count = View::over_retained(stream)
        .child(base, stream.len())?
        .u32_be()?;
    let count_usize = count as usize;
    let (xmt, xmt_len) = read_xmt(stream, base + 4)?;
    let payload = base + 4 + xmt_len;
    let packing = SupportUvPacking::try_from(*stream.get(payload)?).ok()?;
    let values = View::over_retained(stream)
        .child(payload + 1, stream.len())?
        .read_counted(count as u64, 8, View::f64_be)?;
    let values = SupportUvValues::new(packing, values).ok()?;
    Some((
        SupportUvRecord {
            xmt,
            values,
            framing,
            pos,
        },
        payload.checked_add(1 + count_usize.checked_mul(8)?)?,
    ))
}

fn find_tags(stream: &[u8], tag: [u8; 2]) -> impl Iterator<Item = usize> + '_ {
    stream
        .windows(tag.len())
        .enumerate()
        .filter_map(move |(offset, window)| (window == tag).then_some(offset))
}

fn point_m(stream: &[u8], at: usize) -> Option<FinitePoint> {
    let mut view = View::over_retained(stream).child(at, stream.len())?;
    let mm = [
        view.f64_be()? * 1000.0,
        view.f64_be()? * 1000.0,
        view.f64_be()? * 1000.0,
    ];
    FinitePoint::try_from(mm).ok()
}

fn distance(first: Point3, second: Point3) -> f64 {
    ((first.x - second.x).powi(2) + (first.y - second.y).powi(2) + (first.z - second.z).powi(2))
        .sqrt()
}

#[cfg(test)]
mod tests;
