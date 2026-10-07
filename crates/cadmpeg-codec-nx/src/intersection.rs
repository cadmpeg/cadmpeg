// SPDX-License-Identifier: Apache-2.0
//! Decode bounded Parasolid surface-intersection constructions.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::FitTolerance;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::units::FiniteVector;
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
pub(crate) type SupportUv = [Option<SupportUvLane>; 2];

/// Support parameters checked against their chart sample count.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SupportUvLane(Vec<FiniteVector<2>>);

impl SupportUvLane {
    fn present_with_storage<E>(
        values: &[[f64; 2]],
        mut checked: Vec<FiniteVector<2>>,
        mut next: impl FnMut(&mut std::slice::Iter<'_, [f64; 2]>) -> Result<Option<[f64; 2]>, E>,
        mut push: impl FnMut(&mut Vec<FiniteVector<2>>, FiniteVector<2>) -> Result<(), E>,
    ) -> Result<Option<Self>, E> {
        let mut values = values.iter();
        while let Some(pair) = next(&mut values)? {
            if pair.contains(&MISSING_PARAMETER) {
                return Ok(None);
            }
            let Some(pair) = FiniteVector::new(pair) else {
                return Ok(None);
            };
            push(&mut checked, pair)?;
        }
        Ok(Some(Self(checked)))
    }

    pub(crate) fn from_present_values_scoped<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        values: Vec<[f64; 2]>,
    ) -> Result<Option<(Self, cadmpeg_core::decode::ScopedReservation<'ctx>)>, CodecError> {
        let mut reservation = ctx.reserve_scoped(0, "NX chart support-UV lane")?;
        let lane = Self::present_with_storage(
            &values,
            Vec::new(),
            |values| {
                Ok(ctx
                    .next_charged(values, "admit NX chart support-UV lane")?
                    .copied())
            },
            |checked, pair| {
                reservation.with_storage(|| ctx.push_vec(checked, pair, "NX chart support-UV lane"))
            },
        )?;
        Ok(lane.map(|lane| (lane, reservation)))
    }
    /// Construct one parameter pair per chart sample.
    #[cfg(test)]
    pub(crate) fn new(values: Vec<[f64; 2]>, sample_count: usize) -> Option<Self> {
        (values.len() == sample_count).then_some(())?;
        Some(Self(
            values
                .into_iter()
                .map(FiniteVector::new)
                .collect::<Option<Vec<_>>>()?,
        ))
    }

    /// Construct a lane from values already admitted by a source tuple.
    pub(crate) fn from_checked(values: Vec<FiniteVector<2>>, sample_count: usize) -> Option<Self> {
        (values.len() == sample_count).then_some(Self(values))
    }

    pub(crate) fn from_present_values(values: Vec<[f64; 2]>) -> Option<Self> {
        let checked = {
            let mut storage = Vec::new();
            storage.try_reserve_exact(values.len()).map(|()| storage)
        }
        .ok()?;
        match Self::present_with_storage(
            &values,
            checked,
            |values| Ok::<_, Infallible>(values.next().copied()),
            |checked, pair| {
                checked.push(pair);
                Ok(())
            },
        ) {
            Ok(lane) => lane,
            Err(never) => match never {},
        }
    }

    /// Ordered support parameter pairs.
    pub(crate) fn as_slice(&self) -> &[FiniteVector<2>] {
        &self.0
    }
}

impl std::ops::Deref for SupportUvLane {
    type Target = [FiniteVector<2>];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

/// Serialized framing of one `CHART_s` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChartFraming {
    /// Direct `0x0028` tag.
    Direct,
    /// `0x0028ff` escaped tag.
    Escaped,
}

/// Serialized Hvec layout of one `CHART_s` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChartPointLayout {
    /// Three model-space coordinates per point.
    Xyz3,
    /// Eleven scalars containing point, two UV lanes, tangent, and parameter.
    Ext11,
}

/// Serialized framing of one type-59 blend-bound record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BlendBoundFraming {
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
pub(crate) struct ChartSourceRecord {
    /// Cross-reference index of the chart.
    pub(crate) xmt: NonNullXmt,
    /// Checked chart preamble.
    pub(crate) preamble: ChartPreamble,
    /// Points with exactly the fields admitted by their Hvec layout.
    pub(crate) data: SourceChartData,
    /// Serialized record framing.
    pub(crate) framing: ChartFraming,
    /// Type-tag offset in the inflated stream.
    pub(crate) pos: usize,
}

/// A complete type-59 second-support bridge record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlendBound {
    pub(crate) state: BlendBoundState,
    /// Serialized partition/deltas and direct/escaped framing.
    pub(crate) framing: BlendBoundFraming,
    /// Type-tag offset in the inflated stream.
    pub(crate) pos: usize,
}

/// Serialized framing of one `term_use` endpoint record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TermUseFraming {
    /// Direct `0x0029` tag.
    Direct,
    /// `0x0029ff` escaped tag.
    Escaped,
    /// Payload following the inline `term_use` descriptor.
    DescriptorInline,
}

/// Admitted endpoint-form encodings and their required leading counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TermUseForm {
    #[serde(rename = "L?")]
    LQuestion,
    #[serde(rename = "TF")]
    Tf,
    #[serde(rename = "TS")]
    Ts,
}

impl TermUseForm {
    pub(crate) fn count(self) -> u32 {
        match self {
            Self::LQuestion => 1,
            Self::Tf | Self::Ts => 2,
        }
    }
}

/// A complete `term_use` endpoint record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TermUse {
    /// Cross-reference index of the endpoint record.
    pub(crate) xmt: NonNullXmt,
    /// Endpoint form, including its required leading count.
    pub(crate) form: TermUseForm,
    /// Endpoint position in millimetres.
    pub(crate) point: FiniteVector<3>,
    /// Serialized record framing.
    pub(crate) framing: TermUseFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub(crate) pos: usize,
}

/// Serialized framing of one support-UV values array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupportUvFraming {
    /// Direct `0x00cc` tag.
    Direct,
    /// `0x00ccff` escaped tag.
    Escaped,
    /// Payload following the inline `values` descriptor.
    DescriptorInline,
}

/// A complete support-UV values-array record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SupportUvRecord {
    /// Cross-reference index of the values array.
    pub(crate) xmt: NonNullXmt,
    /// Exact finite packed support tuples.
    pub(crate) values: SupportUvValues,
    /// Serialized record framing.
    pub(crate) framing: SupportUvFraming,
    /// Tag or inline-payload offset in the inflated stream.
    pub(crate) pos: usize,
}

/// A decoded surface-intersection construction and its solved chart cache.
#[derive(Debug, Clone)]
pub(crate) struct IntersectionCurve {
    /// Six ordered construction references.
    pub(crate) references: [Option<XmtTarget>; 6],
    /// Cross-reference index of the construction record.
    pub(crate) xmt: u32,
    /// Resolved primary support-surface reference.
    pub(crate) primary_support: NonNullXmt,
    /// Resolved secondary support-surface reference.
    pub(crate) secondary_support: Option<NonNullXmt>,
    /// Type-tag offset of the construction record.
    pub(crate) pos: usize,
    /// Paired chart points in millimetres and native parameters.
    pub(crate) samples: ChartSamples,
    /// Chart chordal error in millimetres.
    pub(crate) fit_tolerance: FitTolerance,
    /// Ordered support UV values in native Parasolid parameter units.
    pub(crate) support_uv: SupportUv,
    /// Two ext11 UV lanes awaiting assignment to the ordered supports.
    pub(crate) ext_support_uv: SupportUv,
}

/// Two distinct non-null support-surface references.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DistinctSupports([NonNullXmt; 2]);

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
pub(crate) struct UnchartedIntersection {
    /// Cross-reference index of the construction record.
    pub(crate) xmt: u32,
    /// Two exact, distinct support-surface references.
    pub(crate) supports: DistinctSupports,
    /// Ordered endpoints of the unique topology edge in millimetres.
    pub(crate) endpoints: [FinitePoint3; 2],
    /// Edge tolerance in millimetres.
    pub(crate) tolerance: PositiveReal,
}

/// Rejection census for structurally decoded intersection constructions whose
/// solved chart carrier is incomplete or inconsistent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RejectionCounts {
    /// The construction did not resolve a valid primary support relation.
    missing_support: usize,
    /// Two construction forms used one stream-local XMT identity.
    duplicate_identity: usize,
    /// The construction's `CHART_s` reference did not resolve to a valid chart.
    pub(crate) missing_chart: usize,
    /// The start term-use reference did not resolve.
    pub(crate) missing_start_term: usize,
    /// The end term-use reference did not resolve.
    pub(crate) missing_end_term: usize,
    /// A term-use endpoint lies outside the chart's chordal-error contract.
    pub(crate) endpoint_mismatch: usize,
}

impl RejectionCounts {
    /// Total rejected construction count.
    pub(crate) fn total(self) -> usize {
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
    pub(crate) fn extend(&mut self, other: Self) {
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
pub(crate) struct CurveScan {
    /// Constructions remaining after cross-form collision selection and before
    /// chart enrichment. Native record extraction reuses this lane
    /// so it does not parse the same graph a second time.
    pub(crate) source_constructions: Vec<CompositeCurve>,
    /// Structurally valid constructions with a solved chart or a typed inbound
    /// curve reference.
    pub(crate) constructions: Vec<CompositeCurve>,
    /// Constructions with a complete solved 3D chart carrier.
    pub(crate) curves: Vec<IntersectionCurve>,
    /// Constructions bounded by exact topology witnesses but lacking a chart.
    pub(crate) uncharted: Vec<UnchartedIntersection>,
    /// Exact rejection census for the remaining parsed constructions.
    pub(crate) rejected: RejectionCounts,
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

enum EnrichError {
    Rejected(Rejection),
    Resource(CodecError),
}

impl From<Rejection> for EnrichError {
    fn from(value: Rejection) -> Self {
        Self::Rejected(value)
    }
}

impl From<CodecError> for EnrichError {
    fn from(value: CodecError) -> Self {
        Self::Resource(value)
    }
}

#[derive(Debug, Clone)]
struct Chart {
    samples: ChartSamples,
    fit_tolerance: FitTolerance,
    ext_support_uv: SupportUv,
}

/// Decode type-38 and single-byte `0x5a` records whose referenced chart and
/// endpoint witnesses form a complete solved cache.
pub(crate) fn curves(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    point_layout: ChartPointLayout,
) -> Result<Vec<IntersectionCurve>, CodecError> {
    Ok(scan(ctx, stream, point_layout)?.curves)
}

/// Decode chart-backed constructions and classify every rejected construction.
fn scan(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    point_layout: ChartPointLayout,
) -> Result<CurveScan, CodecError> {
    let graph = topology::Graph::parse(ctx, stream)?;
    scan_with_graph(ctx, stream, &graph, point_layout)
}

pub(crate) fn scan_with_graph(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    graph: &topology::Graph,
    point_layout: ChartPointLayout,
) -> Result<CurveScan, CodecError> {
    let mut auxiliary_storage = ctx.reserve_scoped(0, "NX intersection auxiliary storage")?;
    let (charts, terms, uv, bridges) = auxiliary_storage.with_storage(|| {
        Ok::<_, CodecError>((
            chart_records(ctx, stream, point_layout)?,
            term_records(ctx, stream)?,
            uv_records(ctx, stream)?,
            blend_bound_records(ctx, stream)?,
        ))
    })?;
    let mut constructions = graph.composite_curves(ctx)?;
    append_intersection_data_curves(ctx, stream, &mut constructions)?;
    scan_with_auxiliaries(
        ctx,
        AuxiliaryMaps {
            charts: &charts,
            terms: &terms,
            uv: &uv,
            bridges: &bridges,
        },
        graph,
        constructions,
        CrossFormCollision::Reject,
    )
}

fn append_intersection_data_curves(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    constructions: &mut Vec<CompositeCurve>,
) -> Result<(), CodecError> {
    let twins = topology::intersection_data_curves(ctx, stream)?;
    ctx.extend_vec(constructions, twins, "NX intersection constructions")?;
    Ok(())
}

/// Decode a merged partition/deltas stream with explicit auxiliary replacement boundaries.
#[cfg(test)]
fn scan_with_auxiliary_replacements(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    base_stream: &[u8],
    replacement_streams: &[&[u8]],
) -> Result<CurveScan, CodecError> {
    let graph = topology::Graph::parse(ctx, stream)?;
    scan_with_auxiliary_replacements_and_graph(
        ctx,
        stream,
        base_stream,
        replacement_streams,
        &graph,
    )
}

pub(crate) fn scan_with_auxiliary_replacements_and_graph(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    base_stream: &[u8],
    replacement_streams: &[&[u8]],
    graph: &topology::Graph,
) -> Result<CurveScan, CodecError> {
    let mut auxiliary_storage = ctx.reserve_scoped(0, "NX replacement auxiliary storage")?;
    let (mut charts, mut terms, mut uv, mut bridges) = auxiliary_storage.with_storage(|| {
        Ok::<_, CodecError>((
            chart_records(ctx, base_stream, ChartPointLayout::Xyz3)?,
            term_records(ctx, base_stream)?,
            uv_records(ctx, base_stream)?,
            blend_bound_records(ctx, base_stream)?,
        ))
    })?;
    for replacement_stream in
        ctx.admit_iter(replacement_streams, "NX auxiliary replacement traversal")?
    {
        auxiliary_storage.with_storage(|| {
            extend_replacement_map(
                ctx,
                &mut charts,
                chart_records(ctx, replacement_stream, ChartPointLayout::Ext11)?,
                "NX replacement chart keys",
            )?;
            extend_replacement_map(
                ctx,
                &mut terms,
                term_records(ctx, replacement_stream)?,
                "NX replacement term keys",
            )?;
            extend_replacement_map(
                ctx,
                &mut uv,
                uv_records(ctx, replacement_stream)?,
                "NX replacement UV keys",
            )?;
            extend_replacement_map(
                ctx,
                &mut bridges,
                blend_bound_records(ctx, replacement_stream)?,
                "NX replacement bridge keys",
            )?;
            Ok::<_, CodecError>(())
        })?;
    }
    let mut constructions = graph.composite_curves(ctx)?;
    append_intersection_data_curves(ctx, stream, &mut constructions)?;
    scan_with_auxiliaries(
        ctx,
        AuxiliaryMaps {
            charts: &charts,
            terms: &terms,
            uv: &uv,
            bridges: &bridges,
        },
        graph,
        constructions,
        CrossFormCollision::PreferDeltaTwin,
    )
}

fn extend_replacement_map<T>(
    ctx: &DecodeContext<'_>,
    target: &mut BTreeMap<u32, T>,
    replacement: BTreeMap<u32, T>,
    operation: &'static str,
) -> Result<(), CodecError> {
    for (xmt, value) in ctx.admit_iter(replacement, operation)? {
        ctx.insert_btree_map(target, xmt, value, operation)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum CrossFormCollision {
    Reject,
    PreferDeltaTwin,
}

#[derive(Clone, Copy)]
struct AuxiliaryMaps<'a> {
    charts: &'a BTreeMap<u32, Chart>,
    terms: &'a BTreeMap<u32, Point3>,
    uv: &'a BTreeMap<u32, SupportUvValues>,
    bridges: &'a BTreeMap<u32, u32>,
}

fn scan_with_auxiliaries(
    ctx: &DecodeContext<'_>,
    maps: AuxiliaryMaps<'_>,
    graph: &topology::Graph,
    constructions: Vec<CompositeCurve>,
    cross_form_collision: CrossFormCollision,
) -> Result<CurveScan, CodecError> {
    let AuxiliaryMaps {
        charts,
        terms,
        uv,
        bridges,
    } = maps;
    let mut reference_storage = ctx.reserve_scoped(0, "NX referenced intersection curve index")?;
    let referenced_curves = reference_storage.with_storage(|| graph.referenced_curve_xmts(ctx))?;
    let mut result = CurveScan::default();
    let mut form_storage = ctx.reserve_scoped(0, "NX intersection form index")?;
    let mut forms_by_xmt = BTreeMap::<u32, u8>::new();
    for construction in ctx.admit_iter(&constructions, "NX intersection form traversal")? {
        form_storage.with_storage(|| {
            let forms = ctx
                .entry_btree_map(
                    &mut forms_by_xmt,
                    construction.xmt,
                    "NX intersection form keys",
                )?
                .or_default();
            *forms |= 1 << u8::from(construction.delta_twin);
            Ok::<_, CodecError>(())
        })?;
    }
    let mut constructions = constructions;
    ctx.retain_vec(
        &mut constructions,
        |construction| {
            if ctx.get_btree_map(
                &forms_by_xmt,
                &construction.xmt,
                "NX intersection cross-form lookup",
            )? == Some(&3)
            {
                match cross_form_collision {
                    CrossFormCollision::Reject => {
                        result.rejected.add(Rejection::DuplicateIdentity);
                        return Ok(false);
                    }
                    CrossFormCollision::PreferDeltaTwin if !construction.delta_twin => {
                        return Ok(false)
                    }
                    CrossFormCollision::PreferDeltaTwin => {}
                }
            }
            Ok(true)
        },
        "NX intersection cross-form selection",
    )?;
    drop(forms_by_xmt);
    drop(form_storage);
    for construction in ctx
        .admit_iter(&constructions, "NX intersection construction traversal")?
        .copied()
    {
        let enriched = enrich(ctx, construction, charts, terms, uv, bridges, graph);
        let referenced = match &enriched {
            Err(EnrichError::Rejected(_)) => ctx.contains_btree_set(
                &referenced_curves,
                &construction.xmt,
                "NX referenced intersection curves",
            )?,
            _ => false,
        };
        match enriched {
            Ok(curve) => {
                ctx.push_vec(
                    &mut result.constructions,
                    construction,
                    "NX intersection constructions",
                )?;
                ctx.push_vec(&mut result.curves, curve, "NX intersection solved curves")?;
            }
            Err(EnrichError::Rejected(rejection)) if referenced => {
                let supports = construction_supports(ctx, construction, uv, bridges, graph)?;
                if supports.is_none()
                    || !construction_has_endpoint_witnesses(ctx, construction, terms, graph)?
                {
                    if matches!(rejection, Rejection::MissingSupport) {
                        result.rejected.add(Rejection::MissingSupport);
                    }
                    continue;
                }
                ctx.push_vec(
                    &mut result.constructions,
                    construction,
                    "NX intersection constructions",
                )?;
                if matches!(rejection, Rejection::MissingChart) {
                    if let (Some(supports), Some((endpoints, tolerance))) = (
                        supports.and_then(|(primary, secondary)| {
                            DistinctSupports::new(primary, secondary?)
                        }),
                        graph
                            .unique_curve_edge_witness(ctx, construction.xmt)?
                            .and_then(|witness| {
                                Some((
                                    witness.endpoints,
                                    PositiveReal::new(witness.tolerance() * 1000.0)?,
                                ))
                            }),
                    ) {
                        ctx.push_vec(
                            &mut result.uncharted,
                            UnchartedIntersection {
                                xmt: construction.xmt,
                                supports,
                                endpoints,
                                tolerance,
                            },
                            "NX uncharted intersections",
                        )?;
                    }
                }
                result.rejected.add(rejection);
            }
            Err(EnrichError::Rejected(_)) => {}
            Err(EnrichError::Resource(error)) => return Err(error),
        }
    }
    result.source_constructions = constructions;
    Ok(result)
}

fn enrich(
    ctx: &DecodeContext<'_>,
    construction: CompositeCurve,
    charts: &BTreeMap<u32, Chart>,
    terms: &BTreeMap<u32, Point3>,
    uv: &BTreeMap<u32, SupportUvValues>,
    bridges: &BTreeMap<u32, u32>,
    graph: &topology::Graph,
) -> Result<IntersectionCurve, EnrichError> {
    let chart = match construction.references[2] {
        Some(target) => {
            ctx.get_btree_map(charts, &u32::from(target), "NX intersection chart lookup")?
        }
        None => None,
    }
    .ok_or(Rejection::MissingChart)?;
    let chart_endpoints = chart.samples.endpoints();
    let term = |reference: Option<XmtTarget>| -> Result<Option<Point3>, CodecError> {
        match reference {
            Some(target) => Ok(ctx
                .get_btree_map(terms, &u32::from(target), "NX intersection term lookup")?
                .copied()),
            None => Ok(None),
        }
    };
    let serialized_terms = [
        term(construction.references[3])?,
        term(construction.references[4])?,
    ];
    if serialized_terms
        .iter()
        .zip(chart_endpoints)
        .any(|(term, endpoint)| {
            term.is_some_and(|term| Point3::distance(term, endpoint) > chart.fit_tolerance.get())
        })
    {
        return Err(Rejection::EndpointMismatch.into());
    }
    if serialized_terms.iter().any(Option::is_none) {
        let topology_endpoints = graph
            .unique_curve_edge_witness(ctx, construction.xmt)?
            .map(|witness| witness.endpoints)
            .ok_or_else(|| {
                if serialized_terms[0].is_none() {
                    Rejection::MissingStartTerm
                } else {
                    Rejection::MissingEndTerm
                }
            })?;
        let matching_permutations = [[0usize, 1usize], [1usize, 0usize]]
            .iter()
            .filter(|permutation| {
                permutation.iter().enumerate().all(|(ordinal, topology)| {
                    Point3::distance(
                        chart_endpoints[ordinal],
                        topology_endpoints[*topology].get(),
                    ) <= chart.fit_tolerance.get()
                })
            })
            .count();
        if matching_permutations != 1 {
            return Err((if serialized_terms[0].is_none() {
                Rejection::MissingStartTerm
            } else {
                Rejection::MissingEndTerm
            })
            .into());
        }
    }
    let (primary_support, secondary_support) =
        construction_supports(ctx, construction, uv, bridges, graph)?
            .ok_or(Rejection::MissingSupport)?;
    let support_values = match construction.references[5] {
        Some(target) => {
            ctx.get_btree_map(uv, &u32::from(target), "NX intersection support-UV lookup")?
        }
        None => None,
    };
    let support_uv = match support_values {
        Some(values) => values.support_uv_charged(ctx, chart.samples.len())?,
        None => [None, None],
    };
    let ext_support_uv = [
        chart.ext_support_uv[0]
            .as_ref()
            .map(|lane| {
                crate::intersection::SupportUvLane::from_checked(
                    ctx.copy_slice(lane.as_slice(), "NX solved support-UV lane copy")?,
                    lane.as_slice().len(),
                )
                .ok_or_else(|| CodecError::malformed("NX copied support-UV lane count"))
            })
            .transpose()?,
        chart.ext_support_uv[1]
            .as_ref()
            .map(|lane| {
                crate::intersection::SupportUvLane::from_checked(
                    ctx.copy_slice(lane.as_slice(), "NX solved support-UV lane copy")?,
                    lane.as_slice().len(),
                )
                .ok_or_else(|| CodecError::malformed("NX copied support-UV lane count"))
            })
            .transpose()?,
    ];
    Ok(IntersectionCurve {
        references: construction.references,
        xmt: construction.xmt,
        primary_support,
        secondary_support,
        pos: construction.pos,
        samples: chart.samples.clone_charged(ctx)?,
        fit_tolerance: chart.fit_tolerance,
        support_uv,
        ext_support_uv,
    })
}

fn construction_supports(
    ctx: &DecodeContext<'_>,
    construction: CompositeCurve,
    uv: &BTreeMap<u32, SupportUvValues>,
    bridges: &BTreeMap<u32, u32>,
    graph: &topology::Graph,
) -> Result<Option<(NonNullXmt, Option<NonNullXmt>)>, CodecError> {
    let (primary, bridge) = if construction.delta_twin {
        (construction.references[0], construction.references[1])
    } else {
        // A present marker-3 values array explicitly reverses the serialized
        // support order. Without that array, retain the type-38 references'
        // order; no alternate order was serialized.
        let packing = match construction.references[5] {
            Some(target) => ctx
                .get_btree_map(
                    uv,
                    &u32::from(target),
                    "NX intersection support packing lookup",
                )?
                .map(SupportUvValues::packing),
            None => None,
        };
        match packing {
            Some(SupportUvPacking::Form3) => {
                (construction.references[1], construction.references[0])
            }
            Some(SupportUvPacking::Form2 | SupportUvPacking::Form4) | None => {
                (construction.references[0], construction.references[1])
            }
        }
    };
    let Some(primary) = primary.map(u32::from) else {
        return Ok(None);
    };
    if !is_surface(ctx, graph, primary)? {
        return Ok(None);
    }
    let secondary = match bridge.map(u32::from) {
        Some(bridge) => {
            match ctx.get_btree_map(bridges, &bridge, "NX intersection bridge lookup")? {
                Some(&secondary) => Some(secondary),
                None => is_surface(ctx, graph, bridge)?.then_some(bridge),
            }
        }
        None => None,
    };
    let secondary = secondary
        .filter(|secondary| *secondary != primary)
        .and_then(|secondary| NonNullXmt::try_from(secondary).ok());
    Ok(NonNullXmt::try_from(primary)
        .ok()
        .map(|primary| (primary, secondary)))
}

fn construction_has_endpoint_witnesses(
    ctx: &DecodeContext<'_>,
    construction: CompositeCurve,
    terms: &BTreeMap<u32, Point3>,
    graph: &topology::Graph,
) -> Result<bool, CodecError> {
    if construction.references[2..=4].iter().all(Option::is_none) {
        return Ok(true);
    }
    let mut witnessed = true;
    for reference in &construction.references[3..=4] {
        let present = match reference {
            Some(target) => ctx.contains_key_btree_map(
                terms,
                &u32::from(*target),
                "NX construction endpoint witnesses",
            )?,
            None => false,
        };
        if !present {
            witnessed = false;
            break;
        }
    }
    Ok(witnessed
        || graph
            .unique_curve_edge_witness(ctx, construction.xmt)?
            .is_some())
}

fn blend_bound_records(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<BTreeMap<u32, u32>, CodecError> {
    let mut records = BTreeMap::new();
    for bound in ctx.admit_iter(&blend_bounds(ctx, stream)?, "NX blend bound traversal")? {
        ctx.insert_btree_map(
            &mut records,
            bound.state.xmt(),
            bound.state.blend_surface(),
            "NX blend-bound map keys",
        )?;
    }
    Ok(records)
}

/// Decode complete type-59 second-support bridge records.
pub(crate) fn blend_bounds(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<BlendBound>, CodecError> {
    let mut out = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    let mut reservation = ctx.reserve_scoped(0, "NX blend-bound record index")?;
    for tag in find_tags(ctx, stream, [0, 59], "NX blend-bound tag traversal")? {
        if let Some((bound, _)) = blend_bound_at(stream, tag) {
            insert_unique_charged(
                ctx,
                &mut reservation,
                &mut out,
                &mut duplicates,
                bound.state.xmt(),
                bound,
                "NX blend-bound identity index",
            )?;
        }
    }
    ctx.collect_vec(out.into_values(), "NX blend-bound records")
}

fn insert_unique_charged<T>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    records: &mut BTreeMap<u32, T>,
    duplicates: &mut BTreeSet<u32>,
    xmt: u32,
    record: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    if ctx.contains_btree_set(duplicates, &xmt, operation)? {
        return Ok(());
    }
    reservation.with_storage(|| {
        if ctx
            .insert_btree_map(records, xmt, record, operation)?
            .is_some()
        {
            ctx.remove_btree_map(records, &xmt, operation)?;
            ctx.insert_btree_set(duplicates, xmt, operation)?;
        }
        Ok(())
    })
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

fn is_surface(
    ctx: &DecodeContext<'_>,
    graph: &topology::Graph,
    xmt: u32,
) -> Result<bool, CodecError> {
    for kind in [
        NodeKind::Plane,
        NodeKind::Cylinder,
        NodeKind::Cone,
        NodeKind::Sphere,
        NodeKind::Torus,
        NodeKind::BlendSurface,
        NodeKind::OffsetSurface,
        NodeKind::BSurface,
    ] {
        if graph.get(ctx, kind, xmt)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn chart_records(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    point_layout: ChartPointLayout,
) -> Result<BTreeMap<u32, Chart>, CodecError> {
    let mut out = BTreeMap::new();
    let mut complemented = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for source in ctx.admit_iter(
        chart_source_records(ctx, stream, point_layout)?,
        "NX source chart traversal",
    )? {
        if ctx.contains_btree_set(
            &duplicates,
            &u32::from(source.xmt),
            "NX duplicate chart lookup",
        )? {
            continue;
        }
        let Some(fit_tolerance) = source.preamble.fit_tolerance() else {
            continue;
        };
        let has_native_parameters = source.data.point_layout() == ChartPointLayout::Ext11;
        let Some((samples, ext_support_uv)) =
            source.data.into_samples_charged(ctx, source.preamble)?
        else {
            continue;
        };
        let candidate = Chart {
            samples,
            fit_tolerance,
            ext_support_uv,
        };
        match ctx.entry_btree_map(&mut out, u32::from(source.xmt), "NX chart identity index")? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let complements = !ctx.contains_btree_set(
                    &complemented,
                    &u32::from(source.xmt),
                    "NX complemented chart lookup",
                )? && has_native_parameters
                    && ctx.all_by(
                        entry
                            .get()
                            .samples
                            .iter_points()
                            .zip(candidate.samples.iter_points()),
                        |(first, second)| {
                            Ok(Point3::distance(first, second)
                                <= entry
                                    .get()
                                    .fit_tolerance
                                    .get()
                                    .max(candidate.fit_tolerance.get()))
                        },
                        "NX chart complement point comparison",
                    )?
                    && entry
                        .get_mut()
                        .samples
                        .replace_parameters_from_charged(ctx, &candidate.samples)?;
                if complements {
                    entry.get_mut().ext_support_uv = candidate.ext_support_uv;
                    ctx.insert_btree_set(
                        &mut complemented,
                        u32::from(source.xmt),
                        "NX complemented chart identities",
                    )?;
                } else {
                    entry.remove();
                    ctx.insert_btree_set(
                        &mut duplicates,
                        u32::from(source.xmt),
                        "NX duplicate chart identities",
                    )?;
                }
            }
        }
    }
    Ok(out)
}

/// Decode every complete physical direct or escaped `CHART_s` source record.
pub(crate) fn chart_source_records(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    point_layout: ChartPointLayout,
) -> Result<Vec<ChartSourceRecord>, CodecError> {
    let mut out = Vec::new();
    let mut tag = 0usize;
    while tag.checked_add(2).is_some_and(|end| end <= stream.len()) {
        ctx.charge_work(1, "scan NX chart records")?;
        if stream.get(tag..tag + 2) == Some(&[0, 40]) {
            if let Some((record, end)) = chart_source_record_at(ctx, stream, tag, point_layout)? {
                ctx.push_vec(&mut out, record, "NX chart source records")?;
                // A complete chart owns its counted point lane. Do not rescan
                // bytes inside that lane as nested chart candidates.
                tag = end;
                continue;
            }
        }
        tag += 1;
    }
    Ok(out)
}

pub(crate) fn chart_source_record_at(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    tag: usize,
    point_layout: ChartPointLayout,
) -> Result<Option<(ChartSourceRecord, usize)>, CodecError> {
    if stream.get(tag..tag + 2) != Some(&[0, 40]) {
        return Ok(None);
    }
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
        let Ok(xmt) = NonNullXmt::try_from(xmt) else {
            continue;
        };
        let preamble = base + 4 + xmt_len;
        let Some(mut head) = View::over_retained(stream).child(preamble, stream.len()) else {
            continue;
        };
        #[rustfmt::skip]
        let (
            Some(base_parameter), Some(base_scale), Some(chart_count), Some(chordal_error),
            Some(angular_error), Some(e0), Some(e1),
        ) = (
            head.f64_be(), head.f64_be(), head.u32_be(), head.f64_be(), head.f64_be(),
            head.f64_be(), head.f64_be(),
        ) else { continue; };
        if cadmpeg_core::decode::index_from_u32(chart_count) != count
            || [e0, e1] != [MISSING_PARAMETER, MISSING_PARAMETER]
        {
            continue;
        }
        let Ok(preamble_values) =
            ChartPreamble::new(base_parameter, base_scale, chordal_error, angular_error)
        else {
            continue;
        };
        let block = preamble + chart_preamble::LEN;
        let Some((data, end)) = chart_points(ctx, stream, block, count, point_layout)? else {
            continue;
        };
        return Ok(Some((
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
        )));
    }
    Ok(None)
}

fn chart_points(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    block: usize,
    count: usize,
    point_layout: ChartPointLayout,
) -> Result<Option<(SourceChartData, usize)>, CodecError> {
    let point_width = match point_layout {
        ChartPointLayout::Xyz3 => 24,
        ChartPointLayout::Ext11 => 88,
    };
    let Some(end) = count
        .checked_mul(point_width)
        .and_then(|bytes| block.checked_add(bytes))
    else {
        return Ok(None);
    };
    if stream.get(block..end).is_none() {
        return Ok(None);
    }
    let count_u64 = cadmpeg_core::decode::u64_from_index(count);
    if point_layout == ChartPointLayout::Xyz3 {
        let operation = "NX raw xyz3 chart points";
        let (mut points, _reservation) = ctx.temporary_vec(count, operation)?;
        let mut indices = 0..count;
        while let Some(index) = ctx.next_charged(&mut indices, "NX chart points range traversal")? {
            let Some(point) = point_m(stream, block + index * 24) else {
                return Ok(None);
            };
            points.push(Point3::from(point.get()));
        }
        return Ok(SourceChartData::xyz3_charged(ctx, points)?.map(|data| (data, end)));
    }

    let operation = "NX raw ext11 chart fields";
    ctx.charge_collection_items(
        count_u64
            .checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, count_u64))?,
        operation,
    )?;
    let (mut points, _point_storage) = ctx.scoped_vector_storage(count, operation)?;
    let (mut native_parameters, _parameter_storage) =
        ctx.scoped_vector_storage(count, operation)?;
    let mut ext_support_uv = [Some(Vec::new()), Some(Vec::new())];
    let mut lane_reservations = [
        ctx.reserve_scoped(0, "NX raw ext11 support-UV lane")?,
        ctx.reserve_scoped(0, "NX raw ext11 support-UV lane")?,
    ];
    let mut indices = 0..count;
    while let Some(index) = ctx.next_charged(&mut indices, "NX chart points range traversal")? {
        let Some((point, parameter, lanes)) = chart_ext_point_at(stream, block + index * 88) else {
            return Ok(None);
        };
        points.push(point);
        native_parameters.push(parameter);
        for lane in 0..2 {
            if lanes[lane]
                .iter()
                .all(|value| value.is_finite() && *value != MISSING_PARAMETER)
            {
                if let Some(values) = &mut ext_support_uv[lane] {
                    ctx.push_scoped_vec(
                        &mut lane_reservations[lane],
                        values,
                        lanes[lane],
                        "NX raw ext11 support-UV lane",
                    )?;
                }
            } else {
                ext_support_uv[lane] = None;
            }
        }
    }
    Ok(
        SourceChartData::ext11_charged(ctx, points, native_parameters, ext_support_uv)?
            .map(|data| (data, end)),
    )
}

fn chart_ext_point_at(stream: &[u8], at: usize) -> Option<(Point3, f64, [[f64; 2]; 2])> {
    let point = Point3::from(point_m(stream, at)?.get());
    let mut mid = View::over_retained(stream).child(at.checked_add(24)?, at.checked_add(88)?)?;
    let (u0, u1, v0, v1) = (mid.f64_be()?, mid.f64_be()?, mid.f64_be()?, mid.f64_be()?);
    let tangent = [mid.f64_be()?, mid.f64_be()?, mid.f64_be()?];
    let parameter = mid.f64_be()?;
    let norm = tangent.iter().map(|v| v * v).sum::<f64>().sqrt();
    let parameter_lanes = [[u0, v0], [u1, v1]];
    ((norm - 1.0).abs() < EPS_INTERSECTION_CHART_POINTS_E9).then_some((
        point,
        parameter,
        parameter_lanes,
    ))
}

fn term_records(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<BTreeMap<u32, Point3>, CodecError> {
    let mut records = BTreeMap::new();
    for term in ctx.admit_iter(&term_use_records(ctx, stream)?, "NX term record traversal")? {
        ctx.insert_btree_map(
            &mut records,
            u32::from(term.xmt),
            Point3::from(term.point.get()),
            "NX term-use map keys",
        )?;
    }
    Ok(records)
}

/// Decode complete direct, escaped, and descriptor-inline `term_use` records.
pub(crate) fn term_use_records(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<TermUse>, CodecError> {
    let mut out = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    let mut reservation = ctx.reserve_scoped(0, "NX term-use record index")?;
    for tag in find_tags(ctx, stream, [0, 41], "NX term-use tag traversal")? {
        if let Some((term, _)) = term_use_at(stream, tag) {
            insert_unique_charged(
                ctx,
                &mut reservation,
                &mut out,
                &mut duplicates,
                u32::from(term.xmt),
                term,
                "NX term-use identity index",
            )?;
        }
    }
    for label in ctx.find_bytes_iter(stream, b"term_use", "scan NX inline term-use records")? {
        let tail = label + b"term_use".len();
        if stream.get(tail..tail + INLINE_TERM_TAIL.len()) == Some(INLINE_TERM_TAIL) {
            let pos = tail + INLINE_TERM_TAIL.len();
            if let Some((term, _)) = term_at(stream, pos, TermUseFraming::DescriptorInline, pos) {
                insert_unique_charged(
                    ctx,
                    &mut reservation,
                    &mut out,
                    &mut duplicates,
                    u32::from(term.xmt),
                    term,
                    "NX term-use identity index",
                )?;
            }
        }
    }
    ctx.collect_vec(out.into_values(), "NX term-use records")
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
    let xmt = NonNullXmt::try_from(xmt).ok()?;
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

fn uv_records(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<BTreeMap<u32, SupportUvValues>, CodecError> {
    let mut records = BTreeMap::new();
    for record in ctx.admit_iter(
        support_uv_records(ctx, stream)?,
        "NX support-UV record traversal",
    )? {
        ctx.insert_btree_map(
            &mut records,
            u32::from(record.xmt),
            record.values,
            "NX support-UV map keys",
        )?;
    }
    Ok(records)
}

/// Decode complete direct, escaped, and descriptor-inline support-UV arrays.
pub(crate) fn support_uv_records(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
) -> Result<Vec<SupportUvRecord>, CodecError> {
    let mut out = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    let mut reservation = ctx.reserve_scoped(0, "NX support-UV record index")?;
    let mut tag = 0usize;
    while tag.checked_add(2).is_some_and(|end| end <= stream.len()) {
        ctx.charge_work(1, "scan NX support-UV records")?;
        if stream.get(tag..tag + 2) == Some(&[0, 204]) {
            let mut value_storage = ctx.reserve_scoped(0, "NX support-UV record payload")?;
            if let Some((record, end)) =
                value_storage.with_storage(|| support_uv_record_at(ctx, stream, tag))?
            {
                insert_unique_charged(
                    ctx,
                    &mut reservation,
                    &mut out,
                    &mut duplicates,
                    u32::from(record.xmt),
                    (record, value_storage),
                    "NX support-UV identity index",
                )?;
                // A complete counted UV lane owns its scalar payload. Do not
                // rescan payload bytes as nested support arrays.
                tag = end;
                continue;
            }
        }
        tag += 1;
    }
    let mut label_start = 0;
    let label_end = stream
        .len()
        .checked_sub(b"values".len())
        .map_or(0, |last| last + 1);
    while label_start < label_end {
        let Some(label) = ctx.find_by(
            label_start..label_end,
            |&at| Ok(stream.get(at..at + b"values".len()) == Some(b"values")),
            "scan NX inline support-UV records",
        )?
        else {
            break;
        };
        let tail = label + b"values".len();
        if stream.get(tail..tail + INLINE_UV_TAIL.len()) == Some(INLINE_UV_TAIL) {
            let pos = tail + INLINE_UV_TAIL.len();
            let mut value_storage = ctx.reserve_scoped(0, "NX support-UV record payload")?;
            if let Some((record, end)) = value_storage
                .with_storage(|| uv_at(ctx, stream, pos, SupportUvFraming::DescriptorInline, pos))?
            {
                insert_unique_charged(
                    ctx,
                    &mut reservation,
                    &mut out,
                    &mut duplicates,
                    u32::from(record.xmt),
                    (record, value_storage),
                    "NX support-UV identity index",
                )?;
                label_start = end;
                continue;
            }
        }
        label_start = label + 1;
    }
    ctx.try_collect_vec(
        out.into_values().map(|(record, storage)| {
            storage.commit()?;
            Ok(record)
        }),
        "NX support-UV records",
    )
}

pub(crate) fn support_uv_record_at(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    tag: usize,
) -> Result<Option<(SupportUvRecord, usize)>, CodecError> {
    if stream.get(tag..tag + 2) != Some(&[0, 204]) {
        return Ok(None);
    }
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
        if let Some(record) = uv_at(ctx, stream, base, framing, tag)? {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

fn uv_at(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    base: usize,
    framing: SupportUvFraming,
    pos: usize,
) -> Result<Option<(SupportUvRecord, usize)>, CodecError> {
    let Some(count) = View::over_retained(stream)
        .child(base, stream.len())
        .and_then(|mut view| view.u32_be())
    else {
        return Ok(None);
    };
    let Ok(count_usize) = usize::try_from(count) else {
        return Ok(None);
    };
    let Some((xmt, xmt_len)) = read_xmt(stream, base + 4) else {
        return Ok(None);
    };
    let Ok(xmt) = NonNullXmt::try_from(xmt) else {
        return Ok(None);
    };
    let Some(payload) = base.checked_add(4).and_then(|at| at.checked_add(xmt_len)) else {
        return Ok(None);
    };
    let Some(packing) = stream
        .get(payload)
        .and_then(|marker| SupportUvPacking::try_from(*marker).ok())
    else {
        return Ok(None);
    };
    let Some(value_start) = payload.checked_add(1) else {
        return Ok(None);
    };
    let Some(value_end) = count_usize
        .checked_mul(8)
        .and_then(|bytes| value_start.checked_add(bytes))
    else {
        return Ok(None);
    };
    let Some(mut view) = View::over_retained(stream).child(value_start, value_end) else {
        return Ok(None);
    };
    let count_u64 = u64::from(count);
    let operation = "NX support-UV scalar lane";
    ctx.charge_collection_items(count_u64, operation)?;
    let (mut scalars, _reservation) = ctx.scoped_vector_storage(count_usize, operation)?;
    for _ in ctx.admit_iter(0..count_usize, "NX support-UV scalar read traversal")? {
        let Some(value) = view.f64_be() else {
            return Ok(None);
        };
        scalars.push(value);
    }
    let Some(values) = SupportUvValues::new_charged(ctx, packing, scalars)? else {
        return Ok(None);
    };
    Ok(Some((
        SupportUvRecord {
            xmt,
            values,
            framing,
            pos,
        },
        value_end,
    )))
}

fn find_tags<'stream>(
    ctx: &DecodeContext<'_>,
    stream: &'stream [u8],
    tag: [u8; 2],
    operation: &'static str,
) -> Result<impl Iterator<Item = usize> + 'stream, CodecError> {
    Ok(ctx
        .admit_iter(stream, operation)?
        .enumerate()
        .skip(1)
        .filter_map(move |(end, _)| (stream[end - 1..=end] == tag).then_some(end - 1)))
}

fn point_m(stream: &[u8], at: usize) -> Option<FiniteVector<3>> {
    let mut view = View::over_retained(stream).child(at, stream.len())?;
    let mm = [
        view.f64_be()? * 1000.0,
        view.f64_be()? * 1000.0,
        view.f64_be()? * 1000.0,
    ];
    FiniteVector::new(mm)
}

#[cfg(test)]
mod tests;
