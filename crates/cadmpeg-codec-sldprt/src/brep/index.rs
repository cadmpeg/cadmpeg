// SPDX-License-Identifier: Apache-2.0
use super::{
    blend, intersection, offset, parse_carrier, sp_curve, spline, subset, sweep, Carrier,
    CurveCarrier, SurfaceCarrier,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::report::loss::LossNote;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// An exact carrier or a derived intersection carrier.
pub(super) enum IndexedCurve {
    Exact(CurveCarrier),
    Derived(intersection::IntersectionCarrier),
}

impl IndexedCurve {
    /// Returns the curve carrier for either provenance variant.
    pub(super) fn carrier(&self) -> &CurveCarrier {
        match self {
            Self::Exact(carrier) => carrier,
            Self::Derived(intersection) => &intersection.carrier,
        }
    }
}

/// The first and last poles of a polynomial NURBS curve.
fn nurbs_ends(
    nurbs: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Option<[cadmpeg_ir::math::Point3; 2]> {
    let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } = nurbs.pole_rows() else {
        return None;
    };
    Some([points.first()?.get(), points.last()?.get()])
}

/// A bounded wrapper of an intersection whose chart is absent: the curve is
/// the intersection of `supports` between the wrapper's two stored ends.
struct TracedWrapper {
    attr: u16,
    offset: usize,
    end: usize,
    supports: [u16; 2],
    ends: [cadmpeg_ir::math::Point3; 2],
}

/// Rolling balls already resolved for one face, keyed by blend attribute and
/// nesting depth.
pub(super) type BlendMemo<'a, 'ctx> =
    RefCell<HashMap<(u16, usize), Option<std::rc::Rc<super::blend_fit::BlendGeometry<'a, 'ctx>>>>>;

/// Blends a rolling-ball support may nest within one another.
const MAX_BLEND_NESTING: usize = 3;

#[derive(Default)]
pub(super) struct CarrierIndex {
    curves: HashMap<u16, IndexedCurve>,
    surfaces: HashMap<u16, SurfaceCarrier>,
    /// Swept/spun surface constructions, resolved to a patch at face binding.
    sweeps: HashMap<u16, sweep::SweepCarrier>,
    /// Constant-radius rolling-ball constructions, resolved at face binding.
    blends: HashMap<u16, blend::BlendCarrier>,
    /// Exact offset-surface constructions, resolved recursively at face binding.
    offsets: HashMap<u16, offset::OffsetCarrier>,
    /// Zero-offset surface pairs referenced by rolling-ball constructions.
    blend_support_pairs: HashMap<u16, blend::SupportPairCarrier>,
    /// Compact curves whose records share one attribute and geometry but not
    /// one orientation marker (spec §7.2).
    marker_conflicts: HashSet<u16>,
    /// Blend-boundary records naming one side of a rolling-ball construction.
    blend_bounds: HashMap<u16, blend::BlendBoundCarrier>,
    /// Intersection records whose chart is absent, with their supports.
    chartless_intersections: HashMap<u16, [u16; 2]>,
    /// Bounded wrappers of chartless intersections, traced at refinement.
    traced: Vec<TracedWrapper>,
    /// The stored chordal error of each intersection chart refined onto its
    /// exact intersection.
    stored_chordal_errors: HashMap<u16, f64>,
    /// `SP_CURVE` records whose `b_curve` decodes (spec §7.6).
    sp_curves: HashMap<u16, sp_curve::SpCurve>,
    /// Bounded curves on an `SP_CURVE`, the curves of a tolerant edge's fins.
    surface_curves: HashMap<u16, sp_curve::SurfaceCurveCarrier>,
    /// Spline carriers whose pole and weight lanes do not pair, each a loss
    /// naming its attribute id and the pairing's own refusal.
    pub(super) lane_refusals: Vec<LossNote>,
}

impl CarrierIndex {
    pub(super) fn insert(&mut self, carrier: Carrier) {
        match carrier {
            Carrier::Curve(carrier) => {
                if let Some(IndexedCurve::Exact(known)) = self.curves.get(&carrier.attr) {
                    if known.geometry == carrier.geometry
                        && known.runs_against_edges != carrier.runs_against_edges
                    {
                        self.marker_conflicts.insert(carrier.attr);
                    }
                }
                self.curves
                    .insert(carrier.attr, IndexedCurve::Exact(carrier));
            }
            Carrier::Surface(carrier) => {
                self.surfaces.insert(carrier.attr, carrier);
            }
        }
    }

    pub(super) fn curve(&self, attr: u16) -> Option<&IndexedCurve> {
        self.curves.get(&attr)
    }

    /// Whether compact records of `attr` disagree on their marker (spec §7.2).
    pub(super) fn marker_conflict(&self, attr: u16) -> bool {
        self.marker_conflicts.contains(&attr)
    }

    pub(super) fn curve_attrs(&self) -> HashSet<u16> {
        self.curves.keys().copied().collect()
    }

    /// The chordal error a refined intersection chart was stored with.
    pub(super) fn stored_chordal_error(&self, attr: u16) -> Option<f64> {
        self.stored_chordal_errors.get(&attr).copied()
    }

    /// The supports of an intersection record whose chart is absent.
    pub(super) fn chartless_intersection(&self, attr: u16) -> Option<[u16; 2]> {
        self.chartless_intersections.get(&attr).copied()
    }

    pub(super) fn surface(&self, attr: u16) -> Option<&SurfaceCarrier> {
        self.surfaces.get(&attr)
    }

    /// Swept/spun surface construction carried by one attribute.
    pub(super) fn sweep(&self, attr: u16) -> Option<&sweep::SweepCarrier> {
        self.sweeps.get(&attr)
    }

    /// Whether any `SP_CURVE` record decoded.
    pub(super) fn has_sp_curves(&self) -> bool {
        !self.sp_curves.is_empty()
    }

    /// The `SP_CURVE` record carried by `attr` (spec §7.6).
    pub(super) fn sp_curve(&self, attr: u16) -> Option<&sp_curve::SpCurve> {
        self.sp_curves.get(&attr)
    }

    /// A bounded curve on an `SP_CURVE` carried by `attr` (spec §7.6).
    pub(super) fn surface_curve(&self, attr: u16) -> Option<&sp_curve::SurfaceCurveCarrier> {
        self.surface_curves.get(&attr)
    }

    /// The support of an `SP_CURVE` on carrier `attr`: a surface carrier,
    /// else a swept or spun construction, else an offset of a surface carrier.
    pub(super) fn sp_curve_support(&self, attr: u16) -> Option<sp_curve::ResolvedSupport<'_>> {
        if let Some(surface) = self.surfaces.get(&attr) {
            return Some(sp_curve::ResolvedSupport::Surface(&surface.geometry));
        }
        self.sweep_support(attr)
            .map(sp_curve::ResolvedSupport::Sweep)
            .or_else(|| {
                self.offset_support(attr)
                    .map(sp_curve::ResolvedSupport::Offset)
            })
    }

    /// The swept or spun construction `attr` as an `SP_CURVE` support: its
    /// profile carrier and its spin or sweep, when the profile decodes.
    pub(super) fn sweep_support(&self, attr: u16) -> Option<sp_curve::SweepSupport> {
        let construction = self.sweeps.get(&attr)?;
        let profile = self.curves.get(&construction.profile_attr)?;
        Some(sp_curve::SweepSupport {
            profile: profile.carrier().geometry.clone(),
            kind: construction.kind.clone(),
        })
    }

    /// The offset `attr` of a surface carrier as an `SP_CURVE` support.
    pub(super) fn offset_support(&self, attr: u16) -> Option<sp_curve::OffsetSupport> {
        let offset = self.offsets.get(&attr)?;
        let base = self.surfaces.get(&offset.support)?;
        Some(sp_curve::OffsetSupport {
            base: base.geometry.clone(),
            distance: offset.distance.get(),
        })
    }

    /// Constant-radius rolling-ball construction carried by `attr`.
    pub(super) fn blend(&self, attr: u16) -> Option<&blend::BlendCarrier> {
        self.blends.get(&attr)
    }

    /// Exact offset-surface construction carried by `attr`.
    pub(super) fn offset(&self, attr: u16) -> Option<&offset::OffsetCarrier> {
        self.offsets.get(&attr)
    }

    /// Zero-offset surface pair carried by `attr`.
    pub(super) fn blend_support_pair(&self, attr: u16) -> Option<&blend::SupportPairCarrier> {
        self.blend_support_pairs.get(&attr)
    }

    /// The wedge of the support pair at `attr` about `spine` over the face
    /// with `vertices`, when both its members are surface carriers.
    ///
    /// # Errors
    ///
    /// Propagates an evaluator's resource refusal.
    pub(super) fn support_pair_wedge<'a>(
        &'a self,
        ctx: &DecodeContext<'_>,
        attr: u16,
        spine: &cadmpeg_ir::geometry::CurveGeometry,
        vertices: &[cadmpeg_ir::math::Point3],
    ) -> Result<Option<super::blend_fit::Wedge<'a>>, cadmpeg_core::CodecError> {
        let Some(pair) = self.blend_support_pairs.get(&attr) else {
            return Ok(None);
        };
        let [Some(first), Some(second)] = pair.supports.map(|member| self.surfaces.get(&member))
        else {
            return Ok(None);
        };
        super::blend_fit::Wedge::new(ctx, spine, [&first.geometry, &second.geometry], vertices)
    }

    /// Refine each degree-one chart onto the exact intersection of its
    /// supports, and trace each chartless wrapper between its ends. A chart
    /// that does not refine keeps its stored points and chordal error.
    ///
    /// # Errors
    ///
    /// Propagates an evaluator's resource refusal.
    pub(super) fn refine_intersections(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let mut refined_curves = Vec::new();
        let mut traced_curves = Vec::new();
        {
            let spines: HashSet<u16> = self.blends.values().map(|blend| blend.spine).collect();
            // Attribute order makes the first refusal deterministic.
            let mut charts = self.curves.iter().collect::<Vec<_>>();
            charts.sort_unstable_by_key(|(attr, _)| **attr);
            for (attr, curve) in charts {
                let IndexedCurve::Derived(intersection) = curve else {
                    continue;
                };
                // A blend's center solve reproduces its spine chart.
                if spines.contains(attr) {
                    continue;
                }
                let [first, second] = intersection.support_data.supports;
                let ends = match &intersection.carrier.geometry {
                    cadmpeg_ir::geometry::CurveGeometry::Solved(
                        cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(stored),
                    ) => nurbs_ends(stored),
                    _ => None,
                };
                let ends = ends.as_ref().map_or(&[][..], |ends| &ends[..]);
                let memo: BlendMemo<'_, '_> = RefCell::new(HashMap::new());
                let blends = [
                    self.blend_geometry(ctx, first, ends, &memo)?,
                    self.blend_geometry(ctx, second, ends, &memo)?,
                ];
                for blend in memo.borrow().values().flatten() {
                    blend.reset_seed();
                }
                let Some(supports) =
                    self.chart_supports(intersection.support_data.supports, &blends)
                else {
                    continue;
                };
                if let Some(refined) =
                    super::chart_refine::refine(&intersection.carrier.geometry, supports)?
                {
                    // Accept a refinement whose ends and departure lie within
                    // the stored chordal error.
                    let tolerance = intersection.support_data.fit_tolerance_mm;
                    let reaches = match &intersection.carrier.geometry {
                        cadmpeg_ir::geometry::CurveGeometry::Solved(
                            cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(stored),
                        ) => match (nurbs_ends(stored), nurbs_ends(&refined.curve)) {
                            (Some(stored), Some(solved)) => {
                                stored[0].distance(solved[0]) <= tolerance
                                    && stored[1].distance(solved[1]) <= tolerance
                            }
                            _ => false,
                        },
                        _ => false,
                    };
                    if reaches && refined.chord_error_mm <= tolerance {
                        refined_curves.push((*attr, refined));
                    }
                }
            }
            for traced in &self.traced {
                let [first, second] = traced.supports;
                let memo: BlendMemo<'_, '_> = RefCell::new(HashMap::new());
                let blends = [
                    self.blend_geometry(ctx, first, &traced.ends, &memo)?,
                    self.blend_geometry(ctx, second, &traced.ends, &memo)?,
                ];
                for blend in memo.borrow().values().flatten() {
                    blend.reset_seed();
                }
                let Some(supports) = self.chart_supports(traced.supports, &blends) else {
                    continue;
                };
                if let Some(refined) = super::chart_refine::trace(supports, traced.ends)? {
                    traced_curves.push((
                        traced.attr,
                        traced.offset,
                        traced.end,
                        traced.supports,
                        refined,
                    ));
                }
            }
        }
        for (attr, refined) in refined_curves {
            if let Some(IndexedCurve::Derived(intersection)) = self.curves.get_mut(&attr) {
                self.stored_chordal_errors
                    .insert(attr, intersection.support_data.fit_tolerance_mm);
                intersection.carrier.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
                    cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(refined.curve),
                );
                intersection.support_data.fit_tolerance_mm = refined
                    .chord_error_mm
                    .max(super::chart_refine::REFINED_CHORD_TOLERANCE_MM);
            }
        }
        for (attr, offset, end, supports, refined) in traced_curves {
            self.curves.entry(attr).or_insert(IndexedCurve::Derived(
                intersection::IntersectionCarrier {
                    carrier: CurveCarrier {
                        attr,
                        offset,
                        end,
                        geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(
                            cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(refined.curve),
                        ),
                        parameter_range: None,
                        runs_against_edges: false,
                    },
                    support_data: intersection::IntersectionSupportData {
                        supports,
                        fit_tolerance_mm: refined
                            .chord_error_mm
                            .max(super::chart_refine::REFINED_CHORD_TOLERANCE_MM),
                        support_uv: None,
                    },
                },
            ));
        }
        Ok(())
    }

    /// The two supports of an intersection: a surface carrier, the pipe of a
    /// blend, or a blend side's contact track that a blend-boundary record
    /// names.
    fn chart_supports<'s, 'g, 'ctx>(
        &'s self,
        attrs: [u16; 2],
        blends: &'s [Option<std::rc::Rc<super::blend_fit::BlendGeometry<'g, 'ctx>>>; 2],
    ) -> Option<[super::chart_refine::Support<'s, 'g, 'ctx>; 2]> {
        let support = |slot: usize| -> Option<super::chart_refine::Support<'s, 'g, 'ctx>> {
            let attr = attrs[slot];
            if let Some(surface) = self.surfaces.get(&attr) {
                return Some(super::chart_refine::Support::Analytic(&surface.geometry));
            }
            let blend = blends[slot].as_deref()?;
            match self.blend_bounds.get(&attr) {
                Some(bound) => Some(super::chart_refine::Support::Track(blend, 1 - bound.side)),
                None => Some(super::chart_refine::Support::Pipe(blend)),
            }
        };
        Some([support(0)?, support(1)?])
    }

    /// The rolling ball of the blend carried by `attr`, or of the blend a
    /// blend-boundary record at `attr` names, over the stretch of its spine
    /// that `vertices` span.
    ///
    /// # Errors
    ///
    /// Propagates an evaluator's resource refusal.
    fn blend_geometry<'a, 'ctx>(
        &'a self,
        ctx: &'a DecodeContext<'ctx>,
        attr: u16,
        vertices: &[cadmpeg_ir::math::Point3],
        memo: &BlendMemo<'a, 'ctx>,
    ) -> Result<
        Option<std::rc::Rc<super::blend_fit::BlendGeometry<'a, 'ctx>>>,
        cadmpeg_core::CodecError,
    > {
        self.nested_blend_geometry(ctx, attr, 0, vertices, memo)
    }

    /// The rolling ball of the blend carried by `attr`, `depth` supports
    /// below the blend asked for, when its spine resolves and each support is
    /// analytic or itself such a blend. A support pair takes its wedge over
    /// the face with `vertices`. `memo` shares each blend's rolling ball
    /// between the supports of that face.
    ///
    /// # Errors
    ///
    /// Propagates an evaluator's resource refusal.
    fn nested_blend_geometry<'a, 'ctx>(
        &'a self,
        ctx: &'a DecodeContext<'ctx>,
        attr: u16,
        depth: usize,
        vertices: &[cadmpeg_ir::math::Point3],
        memo: &BlendMemo<'a, 'ctx>,
    ) -> Result<
        Option<std::rc::Rc<super::blend_fit::BlendGeometry<'a, 'ctx>>>,
        cadmpeg_core::CodecError,
    > {
        if depth > MAX_BLEND_NESTING {
            return Ok(None);
        }
        let attr = self
            .blend_bounds
            .get(&attr)
            .map_or(attr, |bound| bound.blend);
        if let Some(known) = memo.borrow().get(&(attr, depth)) {
            return Ok(known.clone());
        }
        let geometry = self.solve_blend_geometry(ctx, attr, depth, vertices, memo)?;
        memo.borrow_mut().insert((attr, depth), geometry.clone());
        Ok(geometry)
    }

    fn solve_blend_geometry<'a, 'ctx>(
        &'a self,
        ctx: &'a DecodeContext<'ctx>,
        attr: u16,
        depth: usize,
        vertices: &[cadmpeg_ir::math::Point3],
        memo: &BlendMemo<'a, 'ctx>,
    ) -> Result<
        Option<std::rc::Rc<super::blend_fit::BlendGeometry<'a, 'ctx>>>,
        cadmpeg_core::CodecError,
    > {
        let Some(blend) = self.blends.get(&attr) else {
            return Ok(None);
        };
        let Some(spine) = self.curves.get(&blend.spine) else {
            return Ok(None);
        };
        let spine = &spine.carrier().geometry;
        let mut sides: [Vec<super::blend_fit::Side<'a, 'ctx>>; 2] = [Vec::new(), Vec::new()];
        for (slot, support) in blend.supports.iter().enumerate() {
            let (blend::BlendSupportRef::Surface(reference)
            | blend::BlendSupportRef::Pair(reference)) = *support;
            let members = match self.blend_support_pairs.get(&reference) {
                Some(pair) => match pair.supports.map(|member| self.surfaces.get(&member)) {
                    [Some(first), Some(second)] => {
                        let Some(wedge) = super::blend_fit::Wedge::new(
                            ctx,
                            spine,
                            [&first.geometry, &second.geometry],
                            vertices,
                        )?
                        else {
                            return Ok(None);
                        };
                        sides[slot].push(super::blend_fit::Side::Wedge(wedge));
                        continue;
                    }
                    _ => pair.supports.to_vec(),
                },
                None => vec![reference],
            };
            for member in members {
                let Some(side) = self.nested_blend_side(ctx, member, depth, vertices, memo)? else {
                    return Ok(None);
                };
                sides[slot].push(side);
            }
        }
        Ok(
            super::blend_fit::BlendGeometry::new(ctx, spine, sides, blend.signed_radius)
                .map(std::rc::Rc::new),
        )
    }

    /// The support at `attr` as a blend side of the face with `vertices`: its
    /// surface carrier, or the rolling ball of the blend it carries.
    ///
    /// # Errors
    ///
    /// Propagates an evaluator's resource refusal.
    pub(super) fn blend_side<'a, 'ctx>(
        &'a self,
        ctx: &'a DecodeContext<'ctx>,
        attr: u16,
        vertices: &[cadmpeg_ir::math::Point3],
        memo: &BlendMemo<'a, 'ctx>,
    ) -> Result<Option<super::blend_fit::Side<'a, 'ctx>>, cadmpeg_core::CodecError> {
        for blend in memo.borrow().values().flatten() {
            blend.reset_seed();
        }
        self.nested_blend_side(ctx, attr, 0, vertices, memo)
    }

    fn nested_blend_side<'a, 'ctx>(
        &'a self,
        ctx: &'a DecodeContext<'ctx>,
        attr: u16,
        depth: usize,
        vertices: &[cadmpeg_ir::math::Point3],
        memo: &BlendMemo<'a, 'ctx>,
    ) -> Result<Option<super::blend_fit::Side<'a, 'ctx>>, cadmpeg_core::CodecError> {
        match self.surfaces.get(&attr) {
            Some(surface) => Ok(Some(super::blend_fit::Side::Surface(&surface.geometry))),
            None => Ok(self
                .nested_blend_geometry(ctx, attr, depth + 1, vertices, memo)?
                .map(super::blend_fit::Side::Blend)),
        }
    }

    pub(super) fn insert_intersection(&mut self, intersection: intersection::IntersectionCarrier) {
        self.curves
            .entry(intersection.carrier.attr)
            .or_insert(IndexedCurve::Derived(intersection));
    }

    #[cfg(test)]
    pub(super) fn insert_chartless(&mut self, attr: u16, supports: [u16; 2]) {
        self.chartless_intersections.insert(attr, supports);
    }

    #[cfg(test)]
    fn insert_blend_support_pair(&mut self, attr: u16, carrier: blend::SupportPairCarrier) {
        self.blend_support_pairs.insert(attr, carrier);
    }

    pub(super) fn merge_missing(&mut self, other: Self) {
        for attr in other.marker_conflicts {
            if !self.curves.contains_key(&attr) {
                self.marker_conflicts.insert(attr);
            }
        }
        merge_missing_map(&mut self.curves, other.curves);
        merge_missing_map(&mut self.surfaces, other.surfaces);
        merge_missing_map(&mut self.sp_curves, other.sp_curves);
        merge_missing_map(&mut self.surface_curves, other.surface_curves);
        merge_missing_map(&mut self.sweeps, other.sweeps);
        merge_missing_map(&mut self.blends, other.blends);
        merge_missing_map(&mut self.offsets, other.offsets);
        merge_missing_map(&mut self.blend_support_pairs, other.blend_support_pairs);
        merge_missing_map(&mut self.blend_bounds, other.blend_bounds);
        merge_missing_map(
            &mut self.chartless_intersections,
            other.chartless_intersections,
        );
        let mut known = self
            .traced
            .iter()
            .map(|traced| traced.attr)
            .collect::<HashSet<_>>();
        for traced in other.traced {
            if known.insert(traced.attr) {
                self.traced.push(traced);
            }
        }
        self.lane_refusals.extend(other.lane_refusals);
    }
}

fn merge_missing_map<K: Eq + Hash, V>(target: &mut HashMap<K, V>, source: HashMap<K, V>) {
    for (key, value) in source {
        target.entry(key).or_insert(value);
    }
}

/// Scan the whole stream body for compact analytic carriers, keyed by attribute
/// id. A later occurrence in one stream replaces an earlier occurrence at the
/// same identity. Cross-stream precedence is applied by [`CarrierIndex::merge_missing`]:
/// the partition carrier remains authoritative and a deltas carrier fills only
/// an absent identity.
pub(super) fn scan_carriers(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<CarrierIndex, cadmpeg_core::CodecError> {
    let mut out = CarrierIndex::default();
    let mut i = 0usize;
    while i + 2 <= body.len() {
        if body[i] == 0x00 {
            if let Some(c) = parse_carrier(body, i) {
                out.insert(c);
            }
        }
        i += 1;
    }
    let mut lane_refusals = Vec::new();
    for carrier in spline::scan_curve_carriers(body, &mut lane_refusals).into_values() {
        out.insert(Carrier::Curve(carrier));
    }
    for carrier in spline::scan_surface_carriers(body, &mut lane_refusals).into_values() {
        out.insert(Carrier::Surface(carrier));
    }
    for wrapper in subset::scan(ctx, body, &out, subset::Pass::Exact)? {
        if let subset::Wrapper::Curve(carrier) = wrapper {
            out.insert(Carrier::Curve(carrier));
        }
    }
    out.sweeps = sweep::scan_sweep_carriers(body);
    let blend::BlendCarriers {
        blends,
        pairs,
        bounds,
    } = blend::scan(body);
    out.blends = blends;
    out.blend_support_pairs = pairs;
    out.blend_bounds = bounds;
    out.offsets = offset::scan(body);
    for intersection in
        intersection::scan_intersection_carriers(body, &mut lane_refusals).into_values()
    {
        out.insert_intersection(intersection);
    }
    out.chartless_intersections = intersection::scan_chartless_intersections(body);
    for wrapper in subset::scan(ctx, body, &out, subset::Pass::Intersections)? {
        match wrapper {
            subset::Wrapper::Intersection(intersection) => out.insert_intersection(intersection),
            subset::Wrapper::Traced {
                attr,
                offset,
                end,
                supports,
                start,
                finish,
            } => out.traced.push(TracedWrapper {
                attr,
                offset,
                end,
                supports,
                ends: [start, finish],
            }),
            subset::Wrapper::Curve(_) | subset::Wrapper::SurfaceCurve(_) => {}
        }
    }
    // SP_CURVEs read on the supports indexed above.
    if body.windows(2).any(|tag| tag == [0x00, 0x89]) {
        let pcurves = spline::scan_pcurve_carriers(body);
        out.sp_curves = sp_curve::scan(body, &pcurves);
        for wrapper in subset::scan(ctx, body, &out, subset::Pass::SurfaceCurves)? {
            if let subset::Wrapper::SurfaceCurve(carrier) = wrapper {
                out.surface_curves.insert(carrier.attr, *carrier);
            }
        }
    }
    out.lane_refusals = lane_refusals;
    Ok(out)
}

#[cfg(test)]
mod sp_curve_tests;

#[cfg(test)]
mod tests {
    use super::{blend, CarrierIndex};

    #[test]
    fn merge_retains_zero_offset_blend_support_pairs() {
        let mut base = CarrierIndex::default();
        let mut delta = CarrierIndex::default();
        delta.insert_blend_support_pair(
            9,
            blend::SupportPairCarrier {
                supports: [11, 12],
                intersection: 13,
            },
        );

        base.merge_missing(delta);

        let pair = base.blend_support_pair(9).expect("support pair");
        assert_eq!(pair.supports, [11, 12]);
        assert_eq!(pair.intersection, 13);
    }

    #[test]
    fn compact_records_of_one_curve_with_different_markers_conflict() {
        use crate::test_support::parasolid::circle_carrier;

        let circle = |radius: f64, marker: u8| {
            let mut bytes = circle_carrier(70, [0.0; 3], [0.0, 0.0, 1.0], radius);
            bytes[18] = marker;
            bytes
        };
        let scan = |records: &[Vec<u8>]| {
            super::scan_carriers(
                &cadmpeg_test_support::service_decode_context(),
                &records.concat(),
            )
            .expect("carrier scan")
        };
        assert!(scan(&[circle(1.0, 0x2b), circle(1.0, 0x2d)]).marker_conflict(70));
        assert!(!scan(&[circle(1.0, 0x2b), circle(1.0, 0x2b)]).marker_conflict(70));
        assert!(!scan(&[circle(1.0, 0x2b), circle(2.0, 0x2d)]).marker_conflict(70));
    }

    /// A `00 38` rolling-ball record between two direct supports.
    fn blend_record(attr: u16, supports: [u16; 2], spine: u16, radius_m: f64) -> Vec<u8> {
        let mut bytes = vec![0x00, 0x38];
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(&[0; 14]);
        bytes.extend_from_slice(&[0x2b, 0x45]);
        for reference in [supports[0], supports[1], spine] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [radius_m, radius_m, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    /// A degree-one chart on supports 2 and 3 through `poles` in millimetres
    /// at chord-length parameters.
    fn chart(
        attr: u16,
        poles: Vec<cadmpeg_ir::math::Point3>,
        chordal_error: f64,
    ) -> super::intersection::IntersectionCarrier {
        let mut knots = vec![0.0, 0.0];
        let mut length = 0.0;
        for pair in poles.windows(2) {
            length += pair[0].distance(pair[1]);
            knots.push(length);
        }
        knots.push(length);
        super::intersection::IntersectionCarrier {
            carrier: super::CurveCarrier {
                runs_against_edges: false,
                attr,
                offset: 0,
                end: 0,
                geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(
                    cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(
                        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                            1, knots, poles, None, false,
                        )
                        .unwrap(),
                    ),
                ),
                parameter_range: None,
            },
            support_data: super::intersection::IntersectionSupportData {
                supports: [2, 3],
                fit_tolerance_mm: chordal_error,
                support_uv: None,
            },
        }
    }

    fn degree(index: &CarrierIndex, attr: u16) -> u32 {
        match index.curve(attr).unwrap().carrier().geometry.solved() {
            Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(curve)) => curve.degree(),
            _ => panic!("expected a NURBS chart"),
        }
    }

    #[test]
    fn a_chart_on_analytic_supports_refines_when_it_keeps_its_ends() {
        use crate::test_support::parasolid::{cylinder_carrier, plane_carrier};

        // The floor z = 0 meets the cylinder of radius 10 mm about z on the
        // circle of radius 10. Chart 9 runs on that circle; chart 10 ends
        // 0.2 mm off it, beyond its chordal error; chart 11 is the spine of
        // blend 8.
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut index = super::scan_carriers(
            &ctx,
            &[
                plane_carrier(2, [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
                cylinder_carrier(3, [0.0; 3], [0.0, 0.0, 1.0], 0.01),
                blend_record(8, [2, 3], 11, 0.001),
            ]
            .concat(),
        )
        .unwrap();
        let arc = |radius: f64| {
            (0..=8)
                .map(|k| {
                    let angle = std::f64::consts::FRAC_PI_2 * f64::from(k) / 8.0;
                    let r = if k == 8 { radius } else { 10.0 };
                    cadmpeg_ir::math::Point3::new(r * angle.cos(), r * angle.sin(), 0.0)
                })
                .collect::<Vec<_>>()
        };
        index.insert_intersection(chart(9, arc(10.0), 0.05));
        index.insert_intersection(chart(10, arc(10.2), 0.05));
        index.insert_intersection(chart(11, arc(10.0), 0.05));
        index.refine_intersections(&ctx).unwrap();
        assert_eq!(degree(&index, 9), 3);
        assert_eq!(index.stored_chordal_error(9), Some(0.05));
        assert_eq!(
            (degree(&index, 10), index.stored_chordal_error(10)),
            (1, None)
        );
        assert_eq!(
            (degree(&index, 11), index.stored_chordal_error(11)),
            (1, None)
        );
    }

    #[test]
    fn a_blend_boundary_resolves_to_the_rolling_ball_of_its_blend() {
        use crate::test_support::parasolid::{line_carrier, plane_carrier};

        // A 1 mm ball rolls along y between the floor z = 0 and the wall
        // x = 0. Record 21 bounds one side of blend 8.
        let mut bound = vec![0x00, 0x3b];
        bound.extend_from_slice(&21u16.to_be_bytes());
        bound.extend_from_slice(&[0; 14]);
        bound.push(0x2b);
        bound.extend_from_slice(&0u16.to_be_bytes());
        bound.extend_from_slice(&8u16.to_be_bytes());
        let ctx = cadmpeg_test_support::service_decode_context();
        let index = super::scan_carriers(
            &ctx,
            &[
                plane_carrier(2, [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
                plane_carrier(3, [0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                line_carrier(5, [0.001, 0.0, 0.001], [0.0, 1.0, 0.0]),
                blend_record(8, [2, 3], 5, 0.001),
                bound,
            ]
            .concat(),
        )
        .unwrap();
        let memo: super::BlendMemo<'_, '_> =
            std::cell::RefCell::new(std::collections::HashMap::new());
        assert!(index
            .blend_geometry(&ctx, 21, &[], &memo)
            .unwrap()
            .is_some());
        assert!(memo.borrow().contains_key(&(8, 0)));
        assert!(index
            .blend_geometry(&ctx, 22, &[], &memo)
            .unwrap()
            .is_none());
    }

    #[test]
    fn merge_keeps_one_traced_wrapper_per_attribute() {
        let traced = |attr: u16, offset: usize| super::TracedWrapper {
            attr,
            offset,
            end: offset + 70,
            supports: [2, 3],
            ends: [cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 2],
        };
        let mut base = CarrierIndex::default();
        base.traced.push(traced(9, 0));
        let mut delta = CarrierIndex::default();
        delta
            .traced
            .extend([traced(9, 100), traced(10, 200), traced(10, 300)]);
        base.merge_missing(delta);
        assert_eq!(
            base.traced
                .iter()
                .map(|traced| (traced.attr, traced.offset))
                .collect::<Vec<_>>(),
            [(9, 0), (10, 200)]
        );
    }

    #[test]
    fn a_nested_wedge_takes_its_outside_over_the_face_being_fitted() {
        use crate::brep::{Carrier, CurveCarrier, SurfaceCarrier};
        use cadmpeg_ir::geometry::analytic::PlaneSurface;
        use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
        use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
        use cadmpeg_ir::math::{Point3, Vector3};

        // Blend 5 rolls a unit ball between the wedge of the floor z = 0 and
        // the wall x = 0, and the plane x = 1.3. Over y = 0 to 2 its spine
        // runs at x = 0.3, z = 0.95, outside the wall. Its chart runs on to
        // x = -1.15 at y = -60, behind the wall.
        let mut record =
            crate::test_support::parasolid::blend_surface_carrier(5, [9, 20], 7, 1.0e-3);
        record[19] = 0x45;
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut carriers = super::scan_carriers(&ctx, &record).expect("carrier scan");
        let plane = |attr: u16, origin: Point3, normal: Vector3, reference: Vector3| {
            Carrier::Surface(SurfaceCarrier {
                attr,
                offset: 0,
                end: 0,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    PlaneSurface::try_new(origin, normal, reference).expect("plane fixture"),
                )),
                orientation_reversed: false,
            })
        };
        let x = Vector3::new(1.0, 0.0, 0.0);
        let z = Vector3::new(0.0, 0.0, 1.0);
        carriers.insert(plane(11, Point3::new(0.0, 0.0, 0.0), z, x));
        carriers.insert(plane(12, Point3::new(0.0, 0.0, 0.0), x, z));
        carriers.insert(plane(
            20,
            Point3::new(1.3, 0.0, 0.0),
            Vector3::new(-1.0, 0.0, 0.0),
            z,
        ));
        carriers.insert_blend_support_pair(
            9,
            blend::SupportPairCarrier {
                supports: [11, 12],
                intersection: 13,
            },
        );
        carriers.insert(Carrier::Curve(CurveCarrier {
            runs_against_edges: false,
            attr: 7,
            offset: 0,
            end: 0,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                cadmpeg_ir::geometry::nurbs::NurbsCurve::new(
                    1,
                    vec![0.0, 0.0, 0.06, 0.063, 0.063],
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial {
                        points: vec![
                            Point3::new(-1.15, -60.0, 0.95),
                            Point3::new(0.3, 0.0, 0.95),
                            Point3::new(0.3, 3.0, 0.95),
                        ],
                    },
                    false,
                )
                .expect("polyline fixture"),
            )),
            parameter_range: None,
        }));
        let foot = |vertices: &[Point3]| {
            let memo = std::cell::RefCell::new(std::collections::HashMap::new());
            match carriers
                .blend_side(&ctx, 5, vertices, &memo)
                .expect("blend side")
            {
                Some(super::super::blend_fit::Side::Blend(ball)) => ball
                    .pipe_foot(Point3::new(0.3, 1.0, 3.0))
                    .map(|(foot, _)| foot),
                _ => None,
            }
        };
        let near = [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 2.0, 0.0)];
        let far = [Point3::new(-1.0, -58.0, 0.0), Point3::new(-1.1, -59.0, 0.0)];
        let on_face = foot(&near).expect("ball over the near face");
        assert!(
            on_face.distance(Point3::new(0.3, 1.0, 1.0 + 0.91_f64.sqrt())) < 1.0e-6,
            "{on_face:?}"
        );
        assert!(foot(&far).is_none_or(|other| other.distance(on_face) > 1.0e-3));
        let mut both = near.to_vec();
        both.extend_from_slice(&far);
        assert!(foot(&both).is_none_or(|other| other.distance(on_face) > 1.0e-3));
    }
}
