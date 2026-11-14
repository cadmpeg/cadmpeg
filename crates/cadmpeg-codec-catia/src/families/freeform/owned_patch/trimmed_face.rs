// SPDX-License-Identifier: Apache-2.0
//! Nonrectangular loop lifting on source-owned carrier charts.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

mod joint_recovery;
mod shell_edges;

use cadmpeg_core::decode::View;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry, TrimmedPcurve};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::FinitePoint2;

use crate::families::a5a8::records::{a5_owned_rolling_curves, a5_ruled_guides_from_records};
use crate::families::b2::records::{
    b2_circles_from_records, b2_spheres_from_records, owned_cone_charts, owned_edge_nodes,
    owned_parameter_points, owned_torus_charts, B2ParameterPointPayload,
};
use crate::families::chamfer::{ChamferParameters, ChamferResultKind};
use crate::nurbs::LaneRefusals;
use crate::wire::bytes::{allocation_reference, compact_int, AllocationReference};
use crate::wire::records::{
    owned_pcurve_from_record, ConsolidatedFamily as Family, ConsolidatedFrameFlag as Flag,
    ConsolidatedRecord,
};

use super::super::rolling_ball_patch::{fit_edge_with_error, fit_lifted_edge};
use super::super::{consolidated_jet_pcurve, ConsolidatedCarrierChart};
use super::extrusion::{self, Extrusion};
use super::ruled::{self, Ruled};
use super::sheet_carrier::{
    analytic_bridge, carrier_bridge, offset_bridge, offset_cache, Carrier, SpatialCylinder,
    SpatialPlane, SPHERE_BRIDGE_RADIUS_TOLERANCE_MM,
};
use super::stations;
use super::trim_curves;
use super::{
    bridge_radius, curve_point, edge_alignment, interval_parameter, line_pcurve, payload,
    rolling_ball, CadIr, InferredEdgeAccuracy, Member, Patch, Use, GAP_MM,
};

const UV_JOIN: f64 = 1.0e-5;
const TWO_EDGE_RING_JOIN_MM: f64 = 1.0e-4;
const PARALLEL_SINE: f64 = 1.0e-10;
const UNIT_LINE_TOLERANCE: f64 = 1.0e-12;
const ROOT_STEPS: u32 = 32;
const UV_AREA: f64 = 1.0e-10;
const CHART_SLACK: f64 = 1.0e-5;
const ROLLING_TRANSVERSE_CHART_SLACK: f64 = 5.0e-5;
const TRIM_STEPS: u32 = 64;
const MAX_ROLLING_BOUNDARY_EXCURSION: f64 = 0.001;

#[derive(Clone)]
struct Trim {
    edge: usize,
    source: usize,
    vertices: [usize; 2],
    pcurve: PcurveGeometry,
    range: [f64; 2],
    mapped_range: Option<[f64; 2]>,
    affine_map: bool,
    spline_range: Option<[f64; 2]>,
    stations: [Option<f64>; 2],
    primary_stations: [Option<f64>; 2],
    sphere_circle: Option<trim_curves::SphereCircle>,
    alternate_line: Option<LinePcurve>,
    inferred_accuracy: Option<InferredEdgeAccuracy>,
}

impl Trim {
    fn uv(&self, t: f64) -> Option<[f64; 2]> {
        let p = eval::decode::pcurve_uv(EvaluationAdmission::Standard, &self.pcurve, t)
            .ok()?
            .get();
        Some([p.u, p.v])
    }

    fn sample(&self, step: u32, forward: bool) -> f64 {
        let f = f64::from(if forward { step } else { TRIM_STEPS - step }) / f64::from(TRIM_STEPS);
        interval_parameter(self.range, f)
    }
}

pub(super) struct Source<'a, R, P> {
    pub(super) bytes: &'a [u8],
    pub(super) records: &'a [ConsolidatedRecord],
    pub(super) resolve: R,
    pub(super) physical: P,
    pub(super) tensors: &'a BTreeMap<usize, NurbsSurface>,
}

impl<R, P> Source<'_, R, P>
where
    R: Fn(usize, AllocationReference) -> Option<usize>,
    P: Fn(usize) -> Option<usize>,
{
    fn target(&self, index: usize, bytes: &[u8], at: &mut usize, class: u8) -> Option<usize> {
        let target = (self.resolve)(index, allocation_reference(bytes, at)?)?;
        let record = self.records.get(target)?;
        (record.class() == class && (record.family() == Family::B || class == 0x62))
            .then_some(target)
    }

    fn carrier<'a>(
        &'a self,
        ir: &'a CadIr,
        support: usize,
        carriers: &BTreeMap<usize, usize>,
        refusal: &mut LaneRefusals,
    ) -> Option<Carrier<'a>> {
        let record = self.records.get(support)?;
        let bytes = payload(self.bytes, record)?;
        match (record.family(), record.class()) {
            (Family::A, 0x34) => self.tensor(ir, record, carriers),
            (Family::B, 0x2e) => {
                let mut at = 0;
                let reference = allocation_reference(bytes, &mut at)?;
                if at != bytes.len() {
                    return None;
                }
                let target = self.records.get((self.resolve)(support, reference)?)?;
                if target.family() != Family::A || target.class() != 0x34 {
                    return None;
                }
                self.tensor(ir, target, carriers)
            }
            (Family::B, 0x30) => {
                let bridge = offset_bridge(bytes)?;
                let index = (self.resolve)(support, bridge.result)?;
                let target = self.records.get(index)?;
                let bytes = payload(self.bytes, target)?;
                let bounds = bridge.bounds;
                match (bridge.kind, target.family(), target.class()) {
                    (0x15, Family::B, 0x27) => Some(Carrier::Plane(SpatialPlane::from_record(
                        self.bytes, target,
                    )?)),
                    (0x05, Family::B, 0x28) => {
                        Some(Carrier::Cylinder(SpatialCylinder::read(bytes)?))
                    }
                    (0x21, Family::B, 0x2c) => {
                        let chart = Extrusion::read(bytes, bounds)?;
                        self.offset_extrusion(ir, index, &chart, carriers, refusal)
                    }
                    (0x01, Family::B, 0x31) => {
                        let (reference, distance) = offset_cache(bytes, bounds)?;
                        let base_index = (self.resolve)(index, reference)?;
                        let base = self.records.get(base_index)?;
                        if base.family() == Family::A && base.class() == 0x39 {
                            let support_index = (self.resolve)(support, bridge.support)?;
                            let v_limits = self.ruled_support_limits(support_index, base_index)?;
                            let guide = a5_ruled_guides_from_records(
                                self.bytes,
                                std::slice::from_ref(base),
                            )
                            .pop()?;
                            return Some(Carrier::Ruled(Ruled::offset(
                                &guide, v_limits, bounds, distance,
                            )?));
                        }
                        if base.family() != Family::A || base.class() != 0x34 {
                            return None;
                        }
                        let surface = self.tensor_geometry(ir, base, carriers)?;
                        Some(if distance == 0.0 {
                            Carrier::CachedTensor(surface)
                        } else {
                            Carrier::OffsetTensor(surface, bounds, distance)
                        })
                    }
                    (0x19, Family::A, 0x32) => {
                        if bounds[1][0] != 0.0 {
                            return None;
                        }
                        let basis = (self.resolve)(support, bridge.support)?;
                        let chart_end = bounds[1][1];
                        let cached = (self.physical)(target.byte_offset())
                            .and_then(|position| carriers.get(&position));
                        let mut native = None;
                        for distance in [bridge.distance, -bridge.distance] {
                            let Some(radius) = self.rolling_support_radius(basis, distance) else {
                                continue;
                            };
                            if let Some(definition) =
                                cached.and_then(|&i| rolling_ball(ir, i, radius))
                            {
                                return Some(Carrier::OwnedRollingBall {
                                    definition: Box::new(Cow::Borrowed(definition)),
                                    chart_end,
                                    max_rail_shift_mm: 0.0,
                                });
                            }
                            let jet = match &native {
                                Some(jet) => jet,
                                None => native.insert(
                                    a5_owned_rolling_curves(
                                        self.bytes,
                                        std::slice::from_ref(target),
                                    )
                                    .next()?,
                                ),
                            };
                            if !super::near(jet.chart_v_end, chart_end) {
                                return None;
                            }
                            if let Some(carrier) =
                                Carrier::rolling_ball_at_radius(jet.clone(), radius, chart_end)
                            {
                                return Some(carrier);
                            }
                        }
                        None
                    }
                    _ => None,
                }
            }
            (Family::B, 0x27) => Some(Carrier::Plane(SpatialPlane::from_record(
                self.bytes, record,
            )?)),
            (Family::B, 0x28) => Some(Carrier::Cylinder(SpatialCylinder::read(bytes)?)),
            (Family::B, 0x29) => {
                let (cone, reversed) =
                    owned_cone_charts(self.bytes, std::slice::from_ref(record)).next()?;
                Some(Carrier::Cone(cone, reversed))
            }
            (Family::B, 0x2a) => Some(Carrier::Sphere(
                b2_spheres_from_records(self.bytes, std::slice::from_ref(record)).next()?,
            )),
            (Family::B, 0x2b) => {
                let (torus, reversed) =
                    owned_torus_charts(self.bytes, std::slice::from_ref(record)).next()?;
                Some(Carrier::Torus(torus, reversed))
            }
            (Family::B, 0x38) => {
                if let Some(([guide, profile], bounds)) = ruled::bridge(bytes) {
                    let guide = self.records.get((self.resolve)(support, guide)?)?;
                    let profile = self.records.get((self.resolve)(support, profile)?)?;
                    if !matches!(
                        (profile.family(), profile.class()),
                        (Family::A | Family::B, 0x20) | (Family::B, 0x18)
                    ) {
                        return None;
                    }
                    let guide =
                        a5_ruled_guides_from_records(self.bytes, std::slice::from_ref(guide))
                            .pop()?;
                    return Some(Carrier::Ruled(Ruled::new(&guide, bounds[1])?));
                }
                let (reference, class) = carrier_bridge(bytes)?;
                let carrier = (self.resolve)(support, reference)?;
                self.analytic_result(carrier, class)
            }
            (Family::B, 0x3b) => {
                if bytes.first()? != &0x85 {
                    return None;
                }
                let mut at = 1;
                let carrier = (self.resolve)(support, allocation_reference(bytes, &mut at)?)?;
                for _ in 0..2 {
                    let target = (self.resolve)(support, allocation_reference(bytes, &mut at)?)?;
                    if !matches!(self.records[target].class(), 0x27 | 0x29 | 0x38) {
                        return None;
                    }
                }
                for _ in 0..2 {
                    let target = (self.resolve)(support, allocation_reference(bytes, &mut at)?)?;
                    if !matches!(self.records[target].class(), 0x18 | 0x19) {
                        return None;
                    }
                }
                let mut tail = View::over_retained(bytes.get(at..)?);
                let controls = tail.take(6)?.try_into().ok()?;
                let scalars = [
                    FiniteReal::new(tail.f64_le()?)?,
                    FiniteReal::new(tail.f64_le()?)?,
                ];
                if !tail.is_empty() {
                    return None;
                }
                let class = match ChamferParameters::decode(controls, scalars)?.result_kind {
                    ChamferResultKind::Cone => 0x29,
                    ChamferResultKind::Plane => 0x27,
                };
                self.analytic_result(carrier, class)
            }
            (Family::B, 0x37) => {
                if let Some((reference, radius)) = bridge_radius(bytes, true) {
                    let carrier = self.records.get((self.resolve)(support, reference)?)?;
                    if carrier.family() != Family::A || carrier.class() != 0x32 {
                        return None;
                    }
                    if let Some(definition) = (self.physical)(carrier.byte_offset())
                        .and_then(|position| carriers.get(&position))
                        .and_then(|&index| rolling_ball(ir, index, radius))
                    {
                        return Some(Carrier::RollingBall(definition, radius));
                    }
                    let jet = a5_owned_rolling_curves(self.bytes, std::slice::from_ref(carrier))
                        .next()?;
                    let v_end = jet.chart_v_end;
                    return Carrier::rolling_ball_at_radius(jet, radius, v_end);
                }
                let (reference, class, radius) = analytic_bridge(bytes)?;
                let carrier = self.records.get((self.resolve)(support, reference)?)?;
                if carrier.family() != Family::B || carrier.class() != class {
                    return None;
                }
                if class == 0x2b {
                    let (torus, reversed) =
                        owned_torus_charts(self.bytes, std::slice::from_ref(carrier)).next()?;
                    return super::near(torus.minor_radius.get(), radius)
                        .then_some(Carrier::Torus(torus, reversed));
                }
                if class == 0x2a {
                    let sphere = b2_spheres_from_records(self.bytes, std::slice::from_ref(carrier))
                        .next()?;
                    return ((sphere.radius.get() - radius).abs()
                        <= SPHERE_BRIDGE_RADIUS_TOLERANCE_MM)
                        .then_some(Carrier::Sphere(sphere));
                }
                let cylinder = SpatialCylinder::read(payload(self.bytes, carrier)?)?;
                cylinder
                    .matches_radius(radius)
                    .then_some(Carrier::Cylinder(cylinder))
            }
            _ => None,
        }
    }

    fn offset_extrusion(
        &self,
        ir: &CadIr,
        result: usize,
        chart: &Extrusion,
        carriers: &BTreeMap<usize, usize>,
        refusal: &mut LaneRefusals,
    ) -> Option<Carrier<'_>> {
        let record = |index: usize, class| {
            let record = self.records.get(index)?;
            (record.family() == Family::B && record.class() == class).then_some(record)
        };
        let directrix = (self.resolve)(result, chart.directrix)?;
        let (source, offset) = chart.offset(payload(self.bytes, record(directrix, 0x14)?)?)?;
        let map = (self.resolve)(directrix, source)?;
        let [first, second] =
            extrusion::identity_source(payload(self.bytes, record(map, 0x23)?)?, offset.from)?;
        let [first, second] = [first, second].map(|reference| (self.resolve)(map, reference));
        let first = first?;
        let second = second?;
        let curve = owned_pcurve_from_record(self.bytes, self.records.get(first)?)?;
        let companion = owned_pcurve_from_record(self.bytes, self.records.get(second)?)?;
        if curve.range.endpoints() != offset.from || companion.range != curve.range {
            return None;
        }
        let source = payload(self.bytes, self.records.get(first)?)?;
        let base = (self.resolve)(first, allocation_reference(source, &mut 0)?)?;
        if offset_bridge(payload(self.bytes, record(base, 0x30)?)?)?.kind != 0x01 {
            return None;
        }
        let carrier = self.carrier(ir, base, carriers, refusal)?;
        let uv = consolidated_jet_pcurve(&curve, &ConsolidatedCarrierChart::Identity, refusal)?;
        let surface = chart.surface(&offset, |t| {
            let p = eval::decode::pcurve_uv(EvaluationAdmission::Standard, &uv, t)
                .ok()?
                .get();
            carrier.evaluate(clamp_chart(&carrier, [p.u, p.v])?)
        })?;
        Some(Carrier::Extrusion(Box::new(surface)))
    }

    fn ruled_support_limits(&self, mut support: usize, guide: usize) -> Option<[f64; 2]> {
        let mut visited = BTreeSet::new();
        while visited.insert(support) {
            let record = self.records.get(support)?;
            let bytes = payload(self.bytes, record)?;
            match (record.family(), record.class()) {
                (Family::B, 0x38) => {
                    let ([guide_ref, _], bounds) = ruled::bridge(bytes)?;
                    return ((self.resolve)(support, guide_ref)? == guide).then_some(bounds[1]);
                }
                (Family::B, 0x30) => {
                    let bridge = offset_bridge(bytes)?;
                    if bridge.kind != 0x01 {
                        return None;
                    }
                    support = (self.resolve)(support, bridge.support)?;
                }
                _ => return None,
            }
        }
        None
    }

    fn rolling_support_radius(&self, mut support: usize, mut distance: f64) -> Option<f64> {
        let mut visited = BTreeSet::new();
        while visited.insert(support) {
            let record = self.records.get(support)?;
            let bytes = payload(self.bytes, record)?;
            match (record.family(), record.class()) {
                (Family::B, 0x37) => {
                    let (_, radius) = bridge_radius(bytes, true)?;
                    let radius = radius + distance;
                    return (radius.is_finite() && radius > 0.0).then_some(radius);
                }
                (Family::B, 0x30) => {
                    let bridge = offset_bridge(bytes)?;
                    if bridge.kind != 0x19 {
                        return None;
                    }
                    distance += bridge.distance;
                    support = (self.resolve)(support, bridge.support)?;
                }
                _ => return None,
            }
        }
        None
    }

    fn analytic_result(&self, index: usize, class: u8) -> Option<Carrier<'_>> {
        let record = self.records.get(index)?;
        if (record.family(), record.class()) != (Family::B, class) {
            return None;
        }
        match class {
            0x27 => Some(Carrier::Plane(SpatialPlane::from_record(
                self.bytes, record,
            )?)),
            0x29 => {
                let (cone, reversed) =
                    owned_cone_charts(self.bytes, std::slice::from_ref(record)).next()?;
                Some(Carrier::Cone(cone, reversed))
            }
            _ => None,
        }
    }

    fn tensor<'a>(
        &'a self,
        ir: &'a CadIr,
        record: &ConsolidatedRecord,
        carriers: &BTreeMap<usize, usize>,
    ) -> Option<Carrier<'a>> {
        Some(Carrier::Tensor(self.tensor_geometry(ir, record, carriers)?))
    }

    fn tensor_geometry<'a>(
        &'a self,
        ir: &'a CadIr,
        record: &ConsolidatedRecord,
        carriers: &BTreeMap<usize, usize>,
    ) -> Option<&'a NurbsSurface> {
        let cached = (self.physical)(record.byte_offset())
            .and_then(|position| carriers.get(&position))
            .and_then(|&index| ir.model.surfaces.get(index));
        if let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface))) =
            cached.map(|s| &s.geometry)
        {
            return Some(surface);
        }
        self.tensors.get(&record.byte_offset())
    }

    fn trim(
        &self,
        owner: usize,
        bytes: &[u8],
        at: &mut usize,
        carrier: &Carrier<'_>,
        refusal: &mut LaneRefusals,
    ) -> Option<Trim> {
        let p = (self.resolve)(owner, allocation_reference(bytes, at)?)?;
        let edge = self.target(owner, bytes, at, 0x5e)?;
        let node = owned_edge_nodes(self.bytes, &self.records[edge..=edge]).next()?;
        let vertex = |value, encoding| {
            let i = (self.resolve)(edge, AllocationReference { value, encoding })?;
            let record = self.records.get(i)?;
            if record.family() != Family::B || record.class() != 0x5d {
                return None;
            }
            Some(i)
        };
        let vertices = [
            vertex(node.start_vertex_ref, node.reference_encodings[1])?,
            vertex(node.end_vertex_ref, node.reference_encodings[2])?,
        ];
        let records = &self.records[p..=p];
        let mut knots = None;
        let mut extrapolation_sites = 0;
        let mut sphere_circle = None;
        let mut alternate_line = None;
        let (mut pcurve, mut range, source_range) = match self.records[p].class() {
            0x18 => {
                let (points, source_range, alternative) =
                    owned_parameter_points(self.bytes, records)
                        .next()
                        .and_then(|line| {
                            let [a, b] = line.axis_line().or_else(|| {
                                if line.control != 0x0e {
                                    return None;
                                }
                                let B2ParameterPointPayload::Scalar { value } = line.payload else {
                                    return None;
                                };
                                let bounds = match carrier {
                                    Carrier::Plane(plane) => plane.bounds?,
                                    Carrier::Cylinder(_) => carrier.bounds()?,
                                    _ => return None,
                                };
                                let [lo, hi] = bounds[0];
                                Some([[lo, value.get()], [hi, value.get()]])
                            })?;
                            let index = usize::from(line.control & 3 == 1);
                            Some(([a, b], [a[index], b[index]], None))
                        })
                        .or_else(|| {
                            oblique_line(payload(self.bytes, &records[0])?, carrier.bounds()?)
                        })?;
                let line = line_geometry(points)?;
                alternate_line = alternative.and_then(line_geometry);
                (PcurveGeometry::Line(line), [0.0, 1.0], source_range)
            }
            0x20 => {
                let record = owned_pcurve_from_record(self.bytes, &records[0])?;
                let range = record.range.endpoints();
                let decoded_knots = record
                    .sites
                    .iter()
                    .map(|site| site.knot.get())
                    .collect::<Vec<_>>();
                extrapolation_sites = stations::leading_extrapolation_sites(
                    &decoded_knots,
                    record.extrapolation_sites,
                );
                knots = Some(decoded_knots);
                let pcurve =
                    consolidated_jet_pcurve(&record, &ConsolidatedCarrierChart::Identity, refusal)?;
                (pcurve, range, range)
            }
            0x19 => {
                let (pcurve, range) = b2_circles_from_records(self.bytes, records)
                    .next()
                    .and_then(|circle| trim_curves::circle(&circle))
                    .or_else(|| trim_curves::owned_circle(payload(self.bytes, &records[0])?))?;
                (pcurve, range, range)
            }
            0x21 => {
                let (pcurve, range) = trim_curves::spline(payload(self.bytes, &records[0])?)?;
                (pcurve, range, range)
            }
            0x1d => {
                let Carrier::Sphere(sphere) = carrier else {
                    return None;
                };
                let circle = trim_curves::sphere_circle(
                    payload(self.bytes, &records[0])?,
                    sphere.radius.get(),
                )?;
                sphere_circle = Some(circle);
                (circle.fit(circle.range)?, circle.range, circle.range)
            }
            _ => return None,
        };
        let resolve_edge = |lane, value| {
            (self.resolve)(
                edge,
                AllocationReference {
                    value,
                    encoding: node.reference_encodings[lane],
                },
            )
        };
        let inferred_accuracy = (|| {
            let map = resolve_edge(0, node.curve_ref)?;
            let bound_mm =
                stations::mapped_fit_tolerance(self.bytes, self.records, map, p, &self.resolve)?;
            Some(InferredEdgeAccuracy {
                source: (self.physical)(self.records[map].byte_offset())?,
                bound_mm,
            })
        })();
        let selectors = [node.start_parameter_ref, node.end_parameter_ref];
        let spline_range = knots
            .as_ref()
            .and_then(|knots| Some([*knots.first()?, *knots.last()?]));
        let mapped = (|| {
            let map = resolve_edge(0, node.curve_ref)?;
            let (range, affine) =
                stations::mapped_range(self.bytes, self.records, map, p, &self.resolve)?;
            if matches!(pcurve, PcurveGeometry::Line(_)) {
                let mut range =
                    range.map(|t| (t - source_range[0]) / (source_range[1] - source_range[0]));
                if range[0] > range[1] {
                    range.reverse();
                }
                return Some((range, affine));
            }
            let [lo, hi] = spline_range.unwrap_or(source_range);
            if range[0] < lo - CHART_SLACK || range[1] > hi + CHART_SLACK {
                return None;
            }
            let range = [range[0].max(lo), range[1].min(hi)];
            (range[0] < range[1]).then_some((range, affine))
        })();
        let affine_map = mapped.is_some_and(|(_, affine)| affine);
        let mut mapped_range = mapped.map(|(range, _)| range);
        let domain = stations::Domain {
            range: source_range,
            knots: knots.as_deref().and_then(|knots| {
                let first_arc =
                    stations::spline_first_arc(payload(self.bytes, &records[0])?, knots)?;
                Some((knots, first_arc, extrapolation_sites))
            }),
            line: matches!(pcurve, PcurveGeometry::Line(_)),
        };
        let mut primary_stations = [None; 2];
        let mut stations = std::array::from_fn(|endpoint| {
            let index = vertices[endpoint];
            let station = resolve_edge(3 + endpoint, selectors[endpoint]);
            let mapped_station = |primary_station: &mut Option<f64>,
                                  allow_spline_companion,
                                  refusal: &mut LaneRefusals| {
                let map = resolve_edge(0, node.curve_ref)?;
                let (t, primary) = stations::mapped_station(
                    self.bytes,
                    self.records,
                    [index, map, p, station?],
                    allow_spline_companion,
                    &self.resolve,
                    refusal,
                )?;
                let t = if domain.line {
                    t
                } else {
                    let [lo, hi] = spline_range.unwrap_or(source_range);
                    (t >= lo - CHART_SLACK && t <= hi + CHART_SLACK).then_some(t.clamp(lo, hi))?
                };
                if primary {
                    *primary_station = Some(if domain.line {
                        (t - source_range[0]) / (source_range[1] - source_range[0])
                    } else {
                        t
                    });
                }
                Some(t)
            };
            let t = station
                .and_then(|station| {
                    let (t, arc_station) = stations::edge_station(
                        self.bytes,
                        self.records,
                        station,
                        p,
                        domain,
                        &self.resolve,
                        refusal,
                    )?;
                    if arc_station {
                        primary_stations[endpoint] = Some(t);
                    }
                    Some(t)
                })
                .or_else(|| mapped_station(&mut primary_stations[endpoint], domain.line, refusal))
                .or_else(|| {
                    stations::parameter(
                        self.bytes,
                        self.records,
                        index,
                        p,
                        domain,
                        &self.resolve,
                        refusal,
                    )
                })
                .or_else(|| {
                    let map = resolve_edge(0, node.curve_ref)?;
                    if self.records[p].class() == 0x20 {
                        return stations::mapped_spline_parameter(
                            self.bytes,
                            self.records,
                            [index, map, p],
                            source_range,
                            &self.resolve,
                            refusal,
                        );
                    }
                    if self.records[p].class() != 0x19 || !matches!(carrier, Carrier::Plane(_)) {
                        return None;
                    }
                    stations::mapped_circle_endpoint(
                        self.bytes,
                        self.records,
                        [station?, map, p],
                        endpoint,
                        source_range,
                        &self.resolve,
                    )
                })
                .or_else(|| {
                    if domain.line {
                        None
                    } else {
                        mapped_station(&mut primary_stations[endpoint], true, refusal)
                    }
                })?;
            if matches!(pcurve, PcurveGeometry::Line(_)) {
                Some((t - source_range[0]) / (source_range[1] - source_range[0]))
            } else {
                Some(t)
            }
        });
        if domain.line {
            let [lo, hi] = stations
                .into_iter()
                .flatten()
                .fold(range, |[lo, hi], t| [lo.min(t), hi.max(t)]);
            if [lo, hi] != range {
                let extend = |line: &PcurveGeometry| {
                    let points = [lo, hi].map(|t| {
                        eval::decode::pcurve_uv(EvaluationAdmission::Standard, line, t)
                            .ok()
                            .map(|uv| [uv.get().u, uv.get().v])
                    });
                    line_geometry([points[0]?, points[1]?])
                };
                pcurve = PcurveGeometry::Line(extend(&pcurve)?);
                alternate_line =
                    alternate_line.and_then(|line| extend(&PcurveGeometry::Line(line)));
                stations = stations.map(|t| t.map(|t| (t - lo) / (hi - lo)));
                primary_stations = primary_stations.map(|t| t.map(|t| (t - lo) / (hi - lo)));
                mapped_range = mapped_range.map(|range| range.map(|t| (t - lo) / (hi - lo)));
            }
        } else if knots.is_some() {
            range = stations
                .into_iter()
                .flatten()
                .fold(range, |[lo, hi], t| [lo.min(t), hi.max(t)]);
        }
        Some(Trim {
            edge: (self.physical)(node.pos)?,
            source: p,
            vertices: [
                (self.physical)(self.records[vertices[0]].byte_offset())?,
                (self.physical)(self.records[vertices[1]].byte_offset())?,
            ],
            pcurve,
            range,
            mapped_range,
            affine_map,
            spline_range,
            stations,
            primary_stations,
            sphere_circle,
            alternate_line,
            inferred_accuracy,
        })
    }

    pub(super) fn decode<'a>(
        &'a self,
        ir: &'a CadIr,
        face: usize,
        member: Member,
        carriers: &BTreeMap<usize, usize>,
        refusal: &mut LaneRefusals,
    ) -> Option<DecodedFace<'a>> {
        let record = &self.records[face];
        let face_pos = (self.physical)(record.byte_offset())?;
        let bytes = payload(self.bytes, record)?;
        let count = bytes.first()?.checked_sub(0x81)?;
        if count == 0 {
            return None;
        }
        let mut at = 1;
        let support = (self.resolve)(face, allocation_reference(bytes, &mut at)?)?;
        let owner = self.target(face, bytes, &mut at, 0x62)?;
        let inner = (1..count)
            .map(|_| self.target(face, bytes, &mut at, 0x62))
            .collect::<Option<Vec<_>>>()?;
        if !matches!(bytes.get(at..)?, [0x03 | 0x05]) {
            return None;
        }
        let carrier = self.carrier(ir, support, carriers, refusal)?;
        let outer = self.ring(owner, support, &carrier, false, refusal)?;
        let inner = inner
            .into_iter()
            .map(|owner| self.ring(owner, support, &carrier, true, refusal))
            .collect::<Option<Vec<_>>>()?;
        Some(DecodedFace {
            face_pos,
            member,
            carrier,
            outer,
            inner,
        })
    }

    fn ring(
        &self,
        owner: usize,
        support: usize,
        carrier: &Carrier<'_>,
        inner: bool,
        refusal: &mut LaneRefusals,
    ) -> Option<Ring> {
        let record = &self.records[owner];
        if record.flag() != Flag::Flag03 {
            return None;
        }
        let bytes = payload(self.bytes, record)?;
        let (refs, mut at) = match (record.family(), *bytes.first()?) {
            (Family::A, 0x08) => (usize::from(*bytes.get(1)?), 2),
            (Family::A | Family::B, lead) => (usize::from(lead.checked_sub(0x80)?), 1),
        };
        let count = refs / 2;
        if refs % 2 != 1 || count < 2 {
            return None;
        }
        let trims = (0..count)
            .map(|_| self.trim(owner, bytes, &mut at, carrier, refusal))
            .collect::<Option<Vec<_>>>()?;
        if (self.resolve)(owner, allocation_reference(bytes, &mut at)?)? != support {
            return None;
        }
        let tail = bytes.get(at..)?;
        let bounds = ring_tail(tail, count, inner)?.bounds();
        Some(Ring { trims, bounds })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RingBounds {
    chart: [[f64; 2]; 2],
    model: [[f64; 2]; 3],
}

struct Ring {
    trims: Vec<Trim>,
    bounds: Option<RingBounds>,
}

#[derive(Debug, PartialEq)]
enum RingTail {
    Unbounded,
    Bounded(RingBounds),
}

impl RingTail {
    fn bounds(self) -> Option<RingBounds> {
        match self {
            Self::Unbounded => None,
            Self::Bounded(bounds) => Some(bounds),
        }
    }
}

fn ring_tail(bytes: &[u8], count: usize, inner: bool) -> Option<RingTail> {
    let mut view = View::over_retained(bytes);
    if usize::from(view.u8()?) != 0x80 + count
        || !matches!(
            (inner, view.u8()?),
            (true, 0x50 | 0xd0) | (false, 0x41 | 0xc1)
        )
    {
        return None;
    }
    view.skip((3 * count).div_ceil(8))?;
    let bounds = match view.u8()? {
        0x01 => RingTail::Unbounded,
        0x0d if !inner => {
            let lower = [view.f64_le()?, view.f64_le()?];
            let upper = [view.f64_le()?, view.f64_le()?];
            let chart = [
                IncreasingParameterInterval::new([lower[0], upper[0]])?.endpoints(),
                IncreasingParameterInterval::new([lower[1], upper[1]])?.endpoints(),
            ];
            if view.u8()? != 0x01 {
                return None;
            }
            let mut bounds = [[0.0; 2]; 3];
            for axis in &mut bounds {
                let [lo, hi] = [view.f32_le()?, view.f32_le()?];
                if !lo.is_finite() || !hi.is_finite() || lo > hi {
                    return None;
                }
                *axis = [f64::from(lo), f64::from(hi)];
            }
            RingTail::Bounded(RingBounds {
                chart,
                model: bounds,
            })
        }
        _ => return None,
    };
    view.is_empty().then_some(bounds)
}

pub(super) struct DecodedFace<'a> {
    face_pos: usize,
    pub(super) member: Member,
    carrier: Carrier<'a>,
    outer: Ring,
    inner: Vec<Ring>,
}

impl Ring {
    fn contains_vertices(&self, sides: &[Use]) -> bool {
        self.bounds.is_none_or(|bounds| {
            sides.iter().flat_map(|side| side.vertices).all(|(_, p)| {
                [p.x, p.y, p.z]
                    .into_iter()
                    .zip(bounds.model)
                    .all(|(coordinate, [lo, hi])| {
                        coordinate >= lo - GAP_MM && coordinate <= hi + GAP_MM
                    })
            })
        })
    }

    fn patch(
        &self,
        ir: &CadIr,
        carrier: &Carrier<'_>,
        face: usize,
        member: Member,
        vertices: Option<&BTreeMap<usize, Point3>>,
        shared_surface: Option<&SolvedSurfaceGeometry>,
    ) -> Option<Patch> {
        let fit_checked = |trims: &[Trim]| {
            let context = RingFit {
                vertices,
                shared_surface,
                transverse_recovery: TransverseRecovery::None,
            };
            let patch = fit(ir, carrier, face, member, trims, &context)?;
            self.contains_vertices(&patch.sides).then_some(patch)
        };
        fit_checked(&self.trims)
            .or_else(|| {
                let bounds = self.bounds?;
                let mut trims = self.trims.clone();
                let mut changed = false;
                for trim in &mut trims {
                    if matches!(trim.pcurve, PcurveGeometry::Line(_)) {
                        let range = clip_line_range(trim, bounds.chart)?;
                        changed |= range != trim.range;
                        trim.range = range;
                    }
                }
                if changed {
                    fit_checked(&trims)
                } else {
                    None
                }
            })
            .or_else(|| {
                let mut trims = self.trims.clone();
                let mut changed = false;
                for trim in &mut trims {
                    let range = clip_to_carrier(trim, carrier)?;
                    changed |= range != trim.range;
                    trim.range = range;
                }
                changed.then(|| fit_checked(&trims)).flatten()
            })
    }
}

fn clip_to_carrier(trim: &Trim, carrier: &Carrier<'_>) -> Option<[f64; 2]> {
    let inside = |t| trim.uv(t).and_then(|uv| clamp_chart(carrier, uv)).is_some();
    let boundary = |mut out: f64, mut inside_parameter: f64| {
        for _ in 0..ROOT_STEPS {
            let midpoint = out.midpoint(inside_parameter);
            if inside(midpoint) {
                inside_parameter = midpoint;
            } else {
                out = midpoint;
            }
        }
        inside_parameter
    };
    let [lo, hi] = trim.range;
    let mut previous = lo;
    let mut active = inside(lo);
    let mut start = active.then_some(lo);
    let mut end = None;
    for step in 1..=TRIM_STEPS {
        let current = trim.sample(step, true);
        let present = inside(current);
        match (active, present) {
            (false, true) if start.is_none() => start = Some(boundary(previous, current)),
            (false, true) => return None,
            (true, false) => end = Some(boundary(current, previous)),
            _ => (),
        }
        previous = current;
        active = present;
    }
    let range = [start?, end.unwrap_or(hi)];
    (range[0] < range[1]
        && trim
            .stations
            .iter()
            .flatten()
            .all(|&t| (range[0]..=range[1]).contains(&t)))
    .then_some(range)
}

fn clip_line_range(trim: &Trim, chart: [[f64; 2]; 2]) -> Option<[f64; 2]> {
    let a = trim.uv(trim.range[0])?;
    let b = trim.uv(trim.range[1])?;
    let mut fractions = [0.0_f64, 1.0_f64];
    for (axis, [lo, hi]) in chart.into_iter().enumerate() {
        let bounds = [lo - CHART_SLACK, hi + CHART_SLACK];
        let delta = b[axis] - a[axis];
        if delta == 0.0 {
            if a[axis] < bounds[0] || a[axis] > bounds[1] {
                return None;
            }
        } else {
            let [first, last] = bounds.map(|limit| (limit - a[axis]) / delta);
            fractions[0] = fractions[0].max(first.min(last));
            fractions[1] = fractions[1].min(first.max(last));
        }
    }
    let range = fractions.map(|f| interval_parameter(trim.range, f));
    (range[0] < range[1]
        && trim
            .stations
            .iter()
            .flatten()
            .all(|&t| range[0] <= t && t <= range[1]))
    .then_some(range)
}

impl DecodedFace<'_> {
    pub(super) fn endpoint_sites(
        &self,
    ) -> impl Iterator<Item = (usize, usize, Point3, bool, bool)> + '_ {
        std::iter::once(&self.outer)
            .chain(&self.inner)
            .flat_map(|ring| &ring.trims)
            .flat_map(|trim| {
                (0..2).filter_map(|i| {
                    let primary = trim.primary_stations[i];
                    let station = primary.or(trim.stations[i])?;
                    let uv = clamp_chart(&self.carrier, trim.uv(station)?)?;
                    Some((
                        trim.vertices[i],
                        trim.edge,
                        self.carrier.evaluate(uv)?,
                        primary.is_some(),
                        trim.affine_map,
                    ))
                })
            })
    }

    pub(super) fn recover_rolling_boundary(
        &self,
        ir: &CadIr,
        vertices: Option<&BTreeMap<usize, Point3>>,
    ) -> Option<Patch> {
        if !matches!(
            self.carrier,
            Carrier::RollingBall(..) | Carrier::OwnedRollingBall { .. }
        ) || !self.inner.is_empty()
        {
            return None;
        }
        let mut patch = [TransverseRecovery::Clamp, TransverseRecovery::Extend]
            .into_iter()
            .find_map(|transverse_recovery| {
                let context = RingFit {
                    vertices,
                    shared_surface: None,
                    transverse_recovery,
                };
                let patch = fit(
                    ir,
                    &self.carrier,
                    self.face_pos,
                    self.member,
                    &self.outer.trims,
                    &context,
                )?;
                self.outer.contains_vertices(&patch.sides).then_some(patch)
            })?;
        patch.priority = super::sheet_group::ROLLING_BOUNDARY_RETRY_PRIORITY;
        Some(patch)
    }

    pub(super) fn recover_from_shell_edges(&self, edges: &BTreeMap<usize, &Use>) -> Option<Patch> {
        shell_edges::recover(self, edges)
    }

    pub(super) fn patch(
        &self,
        ir: &CadIr,
        vertices: Option<&BTreeMap<usize, Point3>>,
    ) -> Option<Patch> {
        let carrier = &self.carrier;
        let mut patch =
            self.outer
                .patch(ir, carrier, self.face_pos, self.member, vertices, None)?;
        let outer_end = patch.sides.len();
        for ring in &self.inner {
            let member = Member {
                body: self.member.body,
                forward: !self.member.forward,
            };
            let hole = ring.patch(
                ir,
                carrier,
                self.face_pos,
                member,
                vertices,
                Some(&patch.surface),
            )?;
            if !contains_hole(&patch.sides[..outer_end], &hole.sides)?
                || hole.sides.iter().any(|side| {
                    patch.sides[outer_end..].iter().any(|other| {
                        side.edge == other.edge
                            || side.vertices.iter().any(|(id, _)| {
                                other.vertices.iter().any(|(other_id, _)| id == other_id)
                            })
                    })
                })
            {
                return None;
            }
            patch.inner_starts.push(patch.sides.len());
            patch.sides.extend(hole.sides);
        }
        Some(patch)
    }
}

fn clamp_chart(carrier: &Carrier<'_>, [u, v]: [f64; 2]) -> Option<[f64; 2]> {
    let [[lo, hi], [v_lo, v_hi]] = carrier.bounds()?;
    let [v_lo, v_hi] = if matches!(carrier, Carrier::Cylinder(_)) {
        [f64::NEG_INFINITY, f64::INFINITY]
    } else {
        [v_lo, v_hi]
    };
    let v_slack = if matches!(
        carrier,
        Carrier::RollingBall(..) | Carrier::OwnedRollingBall { .. }
    ) {
        ROLLING_TRANSVERSE_CHART_SLACK
    } else {
        CHART_SLACK
    };
    if u < lo - CHART_SLACK || u > hi + CHART_SLACK || v < v_lo - v_slack || v > v_hi + v_slack {
        return None;
    }
    Some(carrier.point([u.clamp(lo, hi), v.clamp(v_lo, v_hi)]))
}

fn recover_rolling_chart(carrier: &Carrier<'_>, [u, v]: [f64; 2]) -> Option<[f64; 2]> {
    let [[u_lo, u_hi], [v_lo, v_hi]] = carrier.bounds()?;
    if u < u_lo - CHART_SLACK || u > u_hi + CHART_SLACK {
        return None;
    }
    let u = u.clamp(u_lo, u_hi);
    let boundary = v.clamp(v_lo, v_hi);
    let excursion = (v - boundary).abs() * carrier.scale()[1];
    if excursion > MAX_ROLLING_BOUNDARY_EXCURSION || carrier.rolling_boundary_gap(u, v)? > GAP_MM {
        return None;
    }
    Some(carrier.point([u, boundary]))
}

type LineEndpoints = [[f64; 2]; 2];

fn line_geometry([a, b]: LineEndpoints) -> Option<LinePcurve> {
    LinePcurve::try_new(
        Point2::new(a[0], a[1]),
        Point2::new(b[0] - a[0], b[1] - a[1]),
    )
    .ok()
}

fn oblique_line(
    bytes: &[u8],
    bounds: [[f64; 2]; 2],
) -> Option<(LineEndpoints, [f64; 2], Option<LineEndpoints>)> {
    let mut at = 0;
    compact_int(bytes, &mut at)?;
    let mut view = View::over_retained(bytes.get(at..)?);
    let control = view.u8()?;
    let lo = view.f64_le()?;
    let hi = match control {
        0 | 0x40 | 0x80 => view.f64_le()?,
        0x08 | 0x48 | 0x88 if lo < 0.0 => -lo,
        _ => return None,
    };
    let origin = [view.f64_le()?, view.f64_le()?];
    let implicit_sign = |axis: usize| {
        let [min, max] = bounds[axis];
        let origin = origin[axis];
        let overlaps = |a: f64, b: f64| (origin + b).min(max) - (origin + a).max(min);
        if overlaps(lo, hi) <= CHART_SLACK && overlaps(-hi, -lo) > CHART_SLACK {
            -1.0
        } else {
            1.0
        }
    };
    let direction = match control {
        0x40 | 0x48 => [0.0, implicit_sign(1)],
        0x80 | 0x88 => [implicit_sign(0), 0.0],
        _ => [view.f64_le()?, view.f64_le()?],
    };
    let norm = direction[0].hypot(direction[1]);
    if !lo.is_finite()
        || !hi.is_finite()
        || lo >= hi
        || !norm.is_finite()
        || norm == 0.0
        || control != 0x00 && (norm - 1.0).abs() > UNIT_LINE_TOLERANCE
        || view.remaining() != 0
    {
        return None;
    }
    let range = [lo, hi];
    let ends = |sign: f64| {
        range.map(|t| {
            let t = t * sign;
            [
                t.mul_add(direction[0], origin[0]),
                t.mul_add(direction[1], origin[1]),
            ]
        })
    };
    Some((
        ends(1.0),
        range,
        matches!(control, 0x40 | 0x80).then(|| ends(-1.0)),
    ))
}

fn cycle<T: Copy>(sides: &[[T; 2]], joins: impl Fn(T, T) -> bool) -> Option<Vec<bool>> {
    if let [first, second] = sides {
        return match (
            joins(first[1], second[0]) && joins(second[1], first[0]),
            joins(first[1], second[1]) && joins(second[0], first[0]),
        ) {
            (true, false) => Some(vec![true, true]),
            (false, true) => Some(vec![true, false]),
            _ => None,
        };
    }
    let mut solution = None;
    for first in [true, false] {
        let mut senses = vec![first];
        let mut end = sides.first()?[usize::from(first)];
        for [a, b] in sides.iter().copied().skip(1) {
            let forward = match (joins(end, a), joins(end, b)) {
                (true, false) => true,
                (false, true) => false,
                _ => break,
            };
            senses.push(forward);
            end = if forward { b } else { a };
        }
        if senses.len() == sides.len()
            && joins(end, sides[0][usize::from(!first)])
            && solution.replace(senses).is_some()
        {
            return None;
        }
    }
    solution
}

fn contains_hole(outer: &[Use], inner: &[Use]) -> Option<bool> {
    let vertices = outer
        .iter()
        .flat_map(|side| side.vertices.map(|v| v.0))
        .collect::<BTreeSet<_>>();
    if inner
        .iter()
        .flat_map(|side| side.vertices)
        .any(|(vertex, _)| vertices.contains(&vertex))
    {
        return Some(false);
    }
    let polygon = |sides: &[Use]| {
        let mut points = Vec::new();
        for side in sides {
            let [lo, hi] = side.curve.full_knot_endpoints().endpoints();
            for i in 0..TRIM_STEPS {
                let fraction = f64::from(if side.forward { i } else { TRIM_STEPS - i })
                    / f64::from(TRIM_STEPS);
                let t = (hi - lo).mul_add(fraction, lo);
                let p = eval::decode::pcurve_uv(EvaluationAdmission::Standard, &side.pcurve, t)
                    .ok()?
                    .get();
                points.push([p.u, p.v]);
            }
        }
        Some(points)
    };
    let outer = polygon(outer)?;
    let inner = polygon(inner)?;
    Some(inner.into_iter().all(|[u, v]| {
        outer
            .iter()
            .zip(outer.iter().cycle().skip(1))
            .fold(false, |inside, (&[a, b], &[c, d])| {
                inside ^ ((b > v) != (d > v) && u < (c - a).mul_add((v - b) / (d - b), a))
            })
    }))
}

pub(super) fn holes_are_contained(patch: &Patch) -> Option<bool> {
    let Some(&outer_end) = patch.inner_starts.first() else {
        return Some(true);
    };
    let outer = &patch.sides[..outer_end];
    let ends = patch
        .inner_starts
        .iter()
        .copied()
        .skip(1)
        .chain([patch.sides.len()]);
    for (&start, end) in patch.inner_starts.iter().zip(ends) {
        if !contains_hole(outer, &patch.sides[start..end])? {
            return Some(false);
        }
    }
    Some(true)
}

fn two_edge_joints(
    trims: &[Trim],
    topological: &[bool],
    known: [Option<[f64; 2]>; 2],
    world: impl Fn([f64; 2]) -> Option<Point3>,
) -> Option<Vec<[f64; 2]>> {
    let [a, b] = trims else { return None };
    let stations = [
        [
            a.stations[usize::from(topological[0])],
            b.stations[usize::from(!topological[1])],
        ],
        [
            b.stations[usize::from(topological[1])],
            a.stations[usize::from(!topological[0])],
        ],
    ];
    if let [Some(a), Some(b)] = known {
        return Some(vec![a, b]);
    }
    if known.iter().all(Option::is_none) && stations.iter().flatten().all(Option::is_none) {
        return None;
    }
    let anchor_a = known[0].or_else(|| joint_recovery::anchored([a, b], stations[0]));
    let anchor_b = known[1].or_else(|| joint_recovery::anchored([b, a], stations[1]));
    let mut solution = None;
    for first in [false, true] {
        for second in [false, true] {
            let ai = usize::from(first);
            let bi = usize::from(second);
            let end_a = anchor_a.unwrap_or([
                stations[0][0].unwrap_or(a.range[ai]),
                stations[0][1].unwrap_or(b.range[1 - bi]),
            ]);
            let end_b = anchor_b.unwrap_or([
                stations[1][0].unwrap_or(b.range[bi]),
                stations[1][1].unwrap_or(a.range[1 - ai]),
            ]);
            let a_delta = end_a[0] - end_b[1];
            let b_delta = end_b[0] - end_a[1];
            if a_delta == 0.0
                || b_delta == 0.0
                || (a_delta > 0.0) != first
                || (b_delta > 0.0) != second
            {
                continue;
            }
            let near = |x, y| {
                world(x)
                    .zip(world(y))
                    .is_some_and(|(x, y)| x.distance(y) <= TWO_EDGE_RING_JOIN_MM)
            };
            if near(a.uv(end_a[0])?, b.uv(end_a[1])?)
                && near(b.uv(end_b[0])?, a.uv(end_b[1])?)
                && solution.replace(vec![end_a, end_b]).is_some()
            {
                return None;
            }
        }
    }
    solution
}

fn line_at(ends: [[f64; 2]; 2], range: [f64; 2], uv: [f64; 2]) -> Option<f64> {
    let [a, b] = ends;
    let d = delta(a, b);
    let length = d[0].hypot(d[1]);
    if length <= 0.0 || !length.is_finite() {
        return None;
    }
    let unit = d.map(|x| x / length);
    let offset = delta(a, uv);
    let t = offset[0].mul_add(unit[0], offset[1] * unit[1]);
    if det(offset, unit).abs() > UV_JOIN || t < -UV_JOIN || t > length + UV_JOIN {
        return None;
    }
    Some(interval_parameter(range, t.clamp(0.0, length) / length))
}

fn delta(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [b[0] - a[0], b[1] - a[1]]
}

fn det(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0].mul_add(b[1], -a[1] * b[0])
}

fn join_at_station(
    sides: [&Trim; 2],
    ends: [[[f64; 2]; 2]; 2],
    stations: [Option<f64>; 2],
) -> Option<[f64; 2]> {
    let locate = |i: usize, uv| {
        if matches!(sides[i].pcurve, PcurveGeometry::Line(_)) {
            return line_at(ends[i], sides[i].range, uv);
        }
        let [a, b] = ends[i].map(|end| {
            let [u, v] = delta(uv, end);
            u.hypot(v) <= UV_JOIN
        });
        match (a, b) {
            (true, false) => Some(sides[i].range[0]),
            (false, true) => Some(sides[i].range[1]),
            _ => None,
        }
    };
    match stations {
        [Some(a), Some(b)] => {
            let [u, v] = delta(sides[0].uv(a)?, sides[1].uv(b)?);
            (u.hypot(v) <= UV_JOIN).then_some([a, b])
        }
        [Some(a), None] => Some([a, locate(1, sides[0].uv(a)?)?]),
        [None, Some(b)] => Some([locate(0, sides[1].uv(b)?)?, b]),
        _ => None,
    }
}

fn join(first: &Trim, second: &Trim, a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> Option<[f64; 2]> {
    let a_line = matches!(first.pcurve, PcurveGeometry::Line(_));
    let b_line = matches!(second.pcurve, PcurveGeometry::Line(_));
    let mut candidates = Vec::new();
    if a_line && b_line {
        let da = delta(a[0], a[1]);
        let db = delta(b[0], b[1]);
        let denominator = det(da, db);
        if denominator.abs() > PARALLEL_SINE * da[0].hypot(da[1]) * db[0].hypot(db[1]) {
            let t = det(delta(a[0], b[0]), db) / denominator;
            let p = [t.mul_add(da[0], a[0][0]), t.mul_add(da[1], a[0][1])];
            return Some([line_at(a, first.range, p)?, line_at(b, second.range, p)?]);
        }
    }
    if a_line && !b_line {
        for (point, t) in b.into_iter().zip(second.range) {
            if let Some(s) = line_at(a, first.range, point) {
                candidates.push([s, t]);
            }
        }
    } else if b_line && !a_line {
        for (point, s) in a.into_iter().zip(first.range) {
            if let Some(t) = line_at(b, second.range, point) {
                candidates.push([s, t]);
            }
        }
    } else {
        for (p, s) in a.into_iter().zip(first.range) {
            for (q, t) in b.into_iter().zip(second.range) {
                if (p[0] - q[0]).hypot(p[1] - q[1]) <= UV_JOIN {
                    candidates.push([s, t]);
                }
            }
        }
    }
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }
    if !candidates.is_empty() {
        return None;
    }
    match (a_line, b_line) {
        (true, false) => line_spline_join(a, first.range, second),
        (false, true) => line_spline_join(b, second.range, first).map(|[a, b]| [b, a]),
        _ => None,
    }
}

fn line_spline_join(line: [[f64; 2]; 2], range: [f64; 2], spline: &Trim) -> Option<[f64; 2]> {
    let direction = delta(line[0], line[1]);
    let distance = |t| Some(det(delta(line[0], spline.uv(t)?), direction));
    let mut candidate = None;
    let mut left = spline.range[0];
    let mut sign = distance(left)?;
    for step in 1..=TRIM_STEPS {
        let right = spline.sample(step, true);
        let end = distance(right)?;
        if sign.is_sign_negative() != end.is_sign_negative() {
            let [mut lo, mut hi] = [left, right];
            for _ in 0..ROOT_STEPS {
                let mid = lo.midpoint(hi);
                if distance(mid)?.is_sign_negative() == sign.is_sign_negative() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let t = lo.midpoint(hi);
            if let Some(s) = line_at(line, range, spline.uv(t)?) {
                if candidate.replace([s, t]).is_some() {
                    return None;
                }
            }
        }
        left = right;
        sign = end;
    }
    candidate
}

fn pole_join(
    carrier: &Carrier<'_>,
    ends: [[[f64; 2]; 2]; 2],
    sides: [&Trim; 2],
) -> Option<[f64; 2]> {
    let Carrier::Sphere(sphere) = carrier else {
        return None;
    };
    let radius = sphere.radius.get();
    let at = |i: usize, v: f64| {
        let [a, b] = ends[i].map(|p| (p[1] - v).abs() <= UV_JOIN);
        match (a, b) {
            (true, false) => Some(sides[i].range[0]),
            (false, true) => Some(sides[i].range[1]),
            _ => None,
        }
    };
    let pole = radius * std::f64::consts::FRAC_PI_2;
    let at_pole = |v| Some([at(0, v)?, at(1, v)?]);
    match (at_pole(pole), at_pole(-pole)) {
        (Some(join), None) | (None, Some(join)) => Some(join),
        _ => None,
    }
}

fn extend_sphere_circles(trims: &mut [Trim], [lo, hi]: [f64; 2]) -> Option<()> {
    let meridians = trims
        .iter()
        .map(|trim| {
            if !matches!(trim.pcurve, PcurveGeometry::Line(_)) {
                return None;
            }
            let [a, b] = [trim.uv(trim.range[0])?, trim.uv(trim.range[1])?];
            ((a[0] - b[0]).abs() <= UV_JOIN && a[0] >= lo && a[0] <= hi).then_some(a[0])
        })
        .collect::<Vec<_>>();
    let count = trims.len();
    for (i, trim) in trims.iter_mut().enumerate() {
        let Some(circle) = trim.sphere_circle else {
            continue;
        };
        let mut range = trim.range;
        for u in [
            meridians[(i + count - 1) % count],
            meridians[(i + 1) % count],
        ]
        .into_iter()
        .flatten()
        {
            range = [range[0].min(u), range[1].max(u)];
        }
        if range != trim.range {
            trim.pcurve = circle.fit(range)?;
            trim.range = range;
        }
    }
    Some(())
}

fn fit(
    ir: &CadIr,
    carrier: &Carrier<'_>,
    face: usize,
    member: Member,
    trims: &[Trim],
    context: &RingFit<'_>,
) -> Option<Patch> {
    let try_fit = |trims: &[Trim], recover| {
        fit_with_alternate_lines(ir, carrier, face, member, trims, context, recover)
    };
    if let Some(patch) = try_fit(trims, false) {
        return Some(patch);
    }
    let mut mapped = trims.to_vec();
    let mut changed = false;
    for trim in &mut mapped {
        if !matches!(trim.pcurve, PcurveGeometry::Line(_)) {
            changed |= apply_mapped_range(trim);
        }
    }
    for trim in &mut mapped {
        if matches!(trim.pcurve, PcurveGeometry::Line(_))
            && trims
                .iter()
                .filter(|other| other.source == trim.source)
                .nth(1)
                .is_some()
        {
            changed |= apply_mapped_range(trim);
        }
    }
    if changed {
        if let Some(patch) = try_fit(&mapped, false) {
            return Some(patch);
        }
    }
    if changed {
        if let Some(patch) = try_fit(&mapped, true) {
            return Some(patch);
        }
    }
    try_fit(trims, true)
}

fn apply_mapped_range(trim: &mut Trim) -> bool {
    let Some(range) = trim.mapped_range.filter(|range| *range != trim.range) else {
        return false;
    };
    trim.range = range;
    trim.stations = trim
        .stations
        .map(|t| t.filter(|t| (range[0]..=range[1]).contains(t)));
    true
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TransverseRecovery {
    None,
    Clamp,
    Extend,
}

struct RingFit<'a> {
    vertices: Option<&'a BTreeMap<usize, Point3>>,
    shared_surface: Option<&'a SolvedSurfaceGeometry>,
    transverse_recovery: TransverseRecovery,
}

fn fit_with_alternate_lines(
    ir: &CadIr,
    carrier: &Carrier<'_>,
    face: usize,
    member: Member,
    trims: &[Trim],
    context: &RingFit<'_>,
    recover: bool,
) -> Option<Patch> {
    const MAX_ALTERNATE_LINES: usize = 4;
    let mut trims = Cow::Borrowed(trims);
    if let (Carrier::Plane(_), Some(vertices)) = (carrier, context.vertices) {
        if let Some(SolvedSurfaceGeometry::Plane(plane)) = carrier.solved([[0.0; 2]; 2], false) {
            for trim in trims.to_mut() {
                shell_edges::select_line(trim, plane, vertices);
            }
        }
    }
    if let Some(patch) = fit_ring(ir, carrier, face, member, trims.to_vec(), context, recover) {
        return Some(patch);
    }
    let choices = trims
        .iter()
        .enumerate()
        .filter_map(|(i, trim)| Some((i, trim.alternate_line?)))
        .collect::<Vec<_>>();
    if choices.len() > MAX_ALTERNATE_LINES {
        return None;
    }
    let mut solution = None;
    for mask in 1..(1usize << choices.len()) {
        let mut ring = trims.to_vec();
        for (lane, (i, line)) in choices.iter().enumerate() {
            if mask & (1 << lane) != 0 {
                ring[*i].pcurve = PcurveGeometry::Line(*line);
            }
        }
        if let Some(patch) = fit_ring(ir, carrier, face, member, ring, context, recover) {
            if solution.replace(patch).is_some() {
                return None;
            }
        }
    }
    solution
}

fn fit_ring(
    ir: &CadIr,
    carrier: &Carrier<'_>,
    face: usize,
    member: Member,
    mut trims: Vec<Trim>,
    context: &RingFit<'_>,
    recover: bool,
) -> Option<Patch> {
    let [lo, hi] = carrier.bounds()?[0];
    let chart = |[u, v]: [f64; 2]| {
        if context.transverse_recovery == TransverseRecovery::Extend {
            if u < lo - CHART_SLACK || u > hi + CHART_SLACK {
                return None;
            }
            let [u, v] = carrier.point([u.clamp(lo, hi), v]);
            carrier.evaluate_rolling_extension(u, v)?;
            return Some([u, v]);
        }
        let uv = [u, v];
        clamp_chart(carrier, uv).or_else(|| {
            (context.transverse_recovery == TransverseRecovery::Clamp)
                .then(|| recover_rolling_chart(carrier, uv))?
        })
    };
    let native_point = |[u, v]: [f64; 2]| {
        if context.transverse_recovery == TransverseRecovery::Extend {
            carrier.evaluate_rolling_extension(u, v)
        } else {
            carrier.evaluate([u, v])
        }
    };
    let identities = trims.iter().map(|trim| trim.vertices).collect::<Vec<_>>();
    let topological = cycle(&identities, |a, b| a == b)?;
    let unique_vertices = identities
        .iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let unique_edges = trims.iter().map(|trim| trim.edge).collect::<BTreeSet<_>>();
    if unique_vertices.len() != trims.len() || unique_edges.len() != trims.len() {
        return None;
    }
    if matches!(carrier, Carrier::Sphere(_)) {
        extend_sphere_circles(&mut trims, [lo, hi])?;
    }
    let ends = trims
        .iter()
        .map(|trim| Some([trim.uv(trim.range[0])?, trim.uv(trim.range[1])?]))
        .collect::<Option<Vec<_>>>()?;
    let witnessed = |i: usize, j: usize| {
        let vertex = trims[i].vertices[usize::from(topological[i])];
        let &point = context.vertices?.get(&vertex)?;
        joint_recovery::known_vertex(
            [&trims[i], &trims[j]],
            point,
            |uv| native_point(chart(uv)?),
            recover,
        )
    };
    let joints = if trims.len() == 2 {
        two_edge_joints(
            &trims,
            &topological,
            [witnessed(0, 1), witnessed(1, 0)],
            |uv| native_point(chart(uv)?),
        )?
    } else {
        (0..trims.len())
            .map(|i| {
                let j = (i + 1) % trims.len();
                let stations = [
                    trims[i].stations[usize::from(topological[i])],
                    trims[j].stations[usize::from(!topological[j])],
                ];
                witnessed(i, j)
                    .or_else(|| {
                        join_at_station([&trims[i], &trims[j]], [ends[i], ends[j]], stations)
                    })
                    .or_else(|| join(&trims[i], &trims[j], ends[i], ends[j]))
                    .or_else(|| pole_join(carrier, [ends[i], ends[j]], [&trims[i], &trims[j]]))
                    .or_else(|| joint_recovery::anchored([&trims[i], &trims[j]], stations))
                    .or_else(|| joint_recovery::crossing([&trims[i], &trims[j]], stations))
                    .or_else(|| {
                        joint_recovery::world_join([&trims[i], &trims[j]], stations, |uv| {
                            native_point(chart(uv)?)
                        })
                    })
                    .or_else(|| {
                        joint_recovery::extended_join(
                            [&trims[i], &trims[j]],
                            [ends[i], ends[j]],
                            stations,
                            |uv| native_point(chart(uv)?),
                        )
                    })
                    .or_else(|| {
                        if !recover {
                            return None;
                        }
                        joint_recovery::extended_splines([&trims[i], &trims[j]], stations, |uv| {
                            native_point(chart(uv)?)
                        })
                    })
            })
            .collect::<Option<Vec<_>>>()?
    };
    let mut chart_order = Vec::new();
    for (index, trim) in trims.iter_mut().enumerate() {
        let entry = joints[(index + joints.len() - 1) % joints.len()][1];
        let exit = joints[index][0];
        if entry == exit {
            return None;
        }
        chart_order.push(entry < exit);
        trim.range = [entry.min(exit), entry.max(exit)];
    }
    let mut points = Vec::new();
    let mut u_min = f64::INFINITY;
    let mut u_max = f64::NEG_INFINITY;
    let mut v_min = f64::INFINITY;
    let mut v_max = f64::NEG_INFINITY;
    let mut rolling_extension_mm = 0.0_f64;
    for (trim, &forward) in trims.iter().zip(&chart_order) {
        for step in 0..=TRIM_STEPS {
            let [u, v] = chart(trim.uv(trim.sample(step, forward))?)?;
            if context.transverse_recovery == TransverseRecovery::Extend {
                rolling_extension_mm =
                    rolling_extension_mm.max(carrier.rolling_boundary_gap(u, v)?);
            }
            u_min = u_min.min(u);
            u_max = u_max.max(u);
            v_min = v_min.min(v);
            v_max = v_max.max(v);
            points.push([u, v]);
        }
    }
    let center = points[0];
    let area = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| {
            (a[0] - center[0]) * (b[1] - center[1]) - (a[1] - center[1]) * (b[0] - center[0])
        })
        .sum::<f64>();
    if area.abs() <= UV_AREA || !area.is_finite() || u_min >= u_max {
        return None;
    }
    let [[u_lo, u_hi], [v_lo, v_hi]] = carrier.bounds()?;
    let [chart_start, chart_end] = [carrier.point([u_lo, v_lo]), carrier.point([u_hi, v_hi])];
    let mut limits = std::array::from_fn(|axis| {
        let [lo, hi] = [[u_min, u_max], [v_min, v_max]][axis];
        [
            (lo - CHART_SLACK).max(chart_start[axis].min(chart_end[axis])),
            (hi + CHART_SLACK).min(chart_start[axis].max(chart_end[axis])),
        ]
    });
    if context.transverse_recovery == TransverseRecovery::Extend {
        if v_min >= 0.0 && v_max <= 1.0 {
            return None;
        }
        limits[1] = [v_min - CHART_SLACK, v_max + CHART_SLACK];
    }
    let solved = match context.shared_surface {
        Some(surface) => surface.clone(),
        None if context.transverse_recovery == TransverseRecovery::Extend => {
            carrier.solve_rolling_extension(limits)?
        }
        None => carrier.solved(limits, recover)?,
    };
    let surface = SurfaceGeometry::Solved(solved.clone());
    let forward = member.forward != carrier.reversed();
    let flip = (area > 0.0) != forward;
    let mut sides = Vec::new();
    for (index, trim) in trims.iter().enumerate() {
        let evaluate = |t| {
            let [u, v] = chart(trim.uv(t)?)?;
            let p = eval::model_surface_point(EvaluationAdmission::Standard, ir, &surface, u, v)
                .ok()?
                .get();
            Some([p.x, p.y, p.z])
        };
        let curve = fit_edge_with_error(trim.range, evaluate, GAP_MM / 4.0)
            .or_else(|| fit_lifted_edge(trim.range, evaluate))?;
        for step in 0..=TRIM_STEPS {
            let t = trim.sample(step, true);
            let [u, v] = chart(trim.uv(t)?)?;
            let p = native_point([u, v])?;
            if p.distance(curve_point(&curve, t)?) > GAP_MM {
                return None;
            }
        }
        let vertices = if topological[index] == chart_order[index] {
            trim.vertices
        } else {
            [trim.vertices[1], trim.vertices[0]]
        };
        let pcurve = match &trim.pcurve {
            PcurveGeometry::Line(_) => line_pcurve(
                trim.range,
                [
                    chart(trim.uv(trim.range[0])?)?,
                    chart(trim.uv(trim.range[1])?)?,
                ],
            )?,
            PcurveGeometry::Nurbs { nurbs } => {
                let mut nurbs = nurbs.clone();
                nurbs
                    .try_map_control_points_in_place(|point| {
                        let p = point.get();
                        let [u, v] = carrier.point([p.u, p.v]);
                        FinitePoint2::new(Point2::new(u, v)).ok_or(())
                    })
                    .ok()?;
                let endpoints = [*nurbs.knots().first()?, *nurbs.knots().last()?];
                let basis = PcurveGeometry::Nurbs { nurbs };
                if trim.range == endpoints {
                    basis
                } else {
                    PcurveGeometry::Trimmed(
                        TrimmedPcurve::try_new(trim.range, true, Box::new(basis)).ok()?,
                    )
                }
            }
            _ => return None,
        };
        let mut side = Use {
            edge: trim.edge,
            vertices: [
                (vertices[0], curve_point(&curve, trim.range[0])?),
                (vertices[1], curve_point(&curve, trim.range[1])?),
            ],
            curve,
            pcurve,
            forward: chart_order[index] != flip,
            inferred_accuracy: trim.inferred_accuracy.filter(|_| {
                trim.mapped_range.is_some_and(|[lo, hi]| {
                    trim.range[0] >= lo - CHART_SLACK && trim.range[1] <= hi + CHART_SLACK
                })
            }),
            lift_error_mm: 0.0,
        };
        if let SolvedSurfaceGeometry::Nurbs(spline) = &solved {
            let tolerance = side
                .inferred_accuracy
                .map_or(GAP_MM, |accuracy| accuracy.bound_mm + GAP_MM);
            let adjusted = (0..=TRIM_STEPS).any(|step| {
                let t = trim.sample(step, true);
                let Some(uv) = trim.uv(t) else {
                    return false;
                };
                if context.transverse_recovery == TransverseRecovery::Clamp
                    && (uv[1] < v_lo || uv[1] > v_hi)
                {
                    return true;
                }
                if !matches!(trim.pcurve, PcurveGeometry::Nurbs { .. })
                    || chart(uv) == Some(carrier.point(uv))
                {
                    return false;
                }
                let [u, v] = carrier.point(uv);
                let point =
                    eval::decode::nurbs_surface_point(EvaluationAdmission::Standard, spline, u, v);
                match (point, curve_point(&side.curve, t)) {
                    (Ok(point), Some(edge)) => point.get().distance(edge) > tolerance,
                    _ => true,
                }
            });
            if adjusted {
                side.pcurve = edge_alignment::project_spline(&side, spline, |fraction| {
                    let t = interval_parameter(trim.range, fraction);
                    let [u, v] = chart(trim.uv(t)?)?;
                    Some(Point2::new(u, v))
                })?;
            }
        }
        sides.push(side);
    }
    if flip {
        sides.reverse();
    }
    Some(Patch {
        face,
        body: member.body,
        forward,
        sense_inferred: carrier.roster_sense_inferred(),
        surface_fit: carrier.surface_fit(),
        surface: solved,
        priority: u8::from(recover) * 8
            + match carrier {
                Carrier::OwnedRollingBall { .. } | Carrier::OffsetTensor(..) => 4,
                Carrier::CachedTensor(..) => 3,
                Carrier::Tensor(..) | Carrier::Extrusion(_) => 2,
                Carrier::Ruled(..) => 2,
                Carrier::Plane(..) => 1,
                _ => 0,
            },
        sides,
        inner_starts: Vec::new(),
        recovery: super::SheetBoundaryRecovery {
            rolling_extension_mm,
            ..super::SheetBoundaryRecovery::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{line_at, ring_tail, RingBounds, RingTail};

    mod recovery;

    #[test]
    fn trim_retains_existing_affine_station_before_companion_fallback() {
        use super::*;
        use crate::wire::bytes::AllocationReferenceEncoding;
        use crate::wire::records::consolidated_records;

        let bytes =
            include_bytes!("../../../../tests/fixtures/endpoint_stations/mapped-priority.bin");
        let records = consolidated_records(bytes);
        let resolve = |index: usize, reference: AllocationReference| {
            let value = usize::try_from(reference.value).ok()?;
            match reference.encoding {
                AllocationReferenceEncoding::OwnedChild => index.checked_add(1)?.checked_add(value),
                AllocationReferenceEncoding::BackwardDistance => index.checked_sub(value),
                _ => None,
            }
        };
        let source = Source {
            bytes,
            records: &records,
            resolve,
            physical: Some,
            tensors: &BTreeMap::new(),
        };
        let ir = CadIr::empty();
        let carriers = BTreeMap::new();
        let mut refusal = LaneRefusals::new();
        let carrier = source
            .carrier(&ir, 10, &carriers, &mut refusal)
            .expect("fixture plane");
        let trim = source
            .trim(0, &[0x0b, 0x17], &mut 0, &carrier, &mut refusal)
            .expect("mapped spline trim");
        assert_eq!(trim.stations, [Some(7.0), Some(7.5)]);
        assert_eq!(trim.primary_stations, [None, None]);
        assert_eq!(trim.mapped_range, Some([0.0, 8.0]));
    }

    #[test]
    fn trim_uses_selector_stations_from_a_spline_companion() {
        use super::*;
        use crate::wire::bytes::AllocationReferenceEncoding;
        use crate::wire::records::consolidated_records;

        let bytes = include_bytes!("../../../../tests/fixtures/endpoint_stations/mapped-trim.bin");
        let records = consolidated_records(bytes);
        let resolve = |index: usize, reference: AllocationReference| {
            let value = usize::try_from(reference.value).ok()?;
            match reference.encoding {
                AllocationReferenceEncoding::OwnedChild => index.checked_add(1)?.checked_add(value),
                AllocationReferenceEncoding::BackwardDistance => index.checked_sub(value),
                _ => None,
            }
        };
        let source = Source {
            bytes,
            records: &records,
            resolve,
            physical: Some,
            tensors: &BTreeMap::new(),
        };
        let ir = CadIr::empty();
        let carriers = BTreeMap::new();
        let mut refusal = LaneRefusals::new();
        let carrier = source
            .carrier(&ir, 10, &carriers, &mut refusal)
            .expect("fixture plane");
        let trim = source
            .trim(0, &[0x0b, 0x17], &mut 0, &carrier, &mut refusal)
            .expect("mapped spline trim");
        assert_eq!(trim.stations, [Some(5.0), Some(6.0)]);
        assert_eq!(trim.primary_stations, [None, None]);
        assert_eq!(trim.mapped_range, Some([0.0, 8.0]));
    }

    #[test]
    fn clipped_line_projection_returns_the_original_parameter() {
        let endpoints = [[2.0, 3.0], [6.0, 3.0]];
        let range = [0.25, 0.75];
        assert_eq!(line_at(endpoints, range, [3.0, 3.0]), Some(0.375));
        assert_eq!(line_at(endpoints, range, [6.0, 3.0]), Some(0.75));
    }

    #[test]
    fn outer_loop_tail_accepts_ordered_numeric_bounds() {
        let mut tail = vec![0x84, 0xc1, 0x9b, 0x0d, 0x0d];
        for parameter in [-1.0_f64, 0.0, 1.0, 2.0] {
            tail.extend(parameter.to_le_bytes());
        }
        tail.push(0x01);
        for bound in [-4.0_f32, 1.0, -3.0, 1.0, 0.0, 2.0] {
            tail.extend(bound.to_le_bytes());
        }
        assert_eq!(
            ring_tail(&tail, 4, false),
            Some(RingTail::Bounded(RingBounds {
                chart: [[-1.0, 1.0], [0.0, 2.0]],
                model: [[-4.0, 1.0], [-3.0, 1.0], [0.0, 2.0]],
            }))
        );
        assert_eq!(ring_tail(&tail, 4, true), None);
        assert_eq!(ring_tail(&tail[..tail.len() - 1], 4, false), None);
    }
}
