// SPDX-License-Identifier: Apache-2.0
//! The carriers of a Brep scope in the forms a 3DM Brep holds.
//!
//! Each edge curve becomes a line with a unit direction or a clamped NURBS
//! curve over the edge's range, each face surface a plane or a clamped NURBS
//! patch, and each pcurve a line or a clamped NURBS curve over its own
//! interval in its face's surface parameters. Every conversion is exact
//! (see [`super::exact`]). A face whose carriers have no such form is left
//! out with the reason; its records go with it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::procedural_curves::{
    CompoundCurveConstruction, ProceduralCurveDefinition,
};
use cadmpeg_ir::geometry::{
    CompositeCurveSegments, Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{Coedge, Edge};

use super::exact::{self, ParameterMap, Spline};
use super::EPS_WRITE_DEGENERATE;

/// A Brep scope with its carriers in 3DM form.
pub(super) struct ConvertedScope {
    pub(super) ir: CadIr,
    pub(super) findings: ScopeFindings,
}

/// Each face the writer leaves out of a Brep scope with the reason, each edge
/// it writes as one edge for each use, each edge whose curve pieces it
/// joins across a gap, with the widest gap, and each edge it writes as
/// several edges.
pub(super) struct ScopeFindings {
    pub(super) dropped: Vec<(String, String)>,
    pub(super) unjoined: Vec<(Unjoin, String)>,
    pub(super) rejoined: Vec<(String, f64)>,
    pub(super) split: Vec<EdgeSplit>,
}

/// The solved form of `geometry`: its solved carrier, or the cache of its
/// procedural record.
fn solved_curve(geometry: &CurveGeometry) -> Option<&SolvedCurveGeometry> {
    match geometry {
        CurveGeometry::Solved(solved) => Some(solved),
        CurveGeometry::Procedural { cache, .. } => cache.as_ref(),
    }
}

fn solved_surface(geometry: &SurfaceGeometry) -> Option<&SolvedSurfaceGeometry> {
    match geometry {
        SurfaceGeometry::Solved(solved) => Some(solved),
        SurfaceGeometry::Procedural { cache, .. } => cache.as_ref(),
    }
}

fn knot_domain(curve: &NurbsCurve) -> Option<[f64; 2]> {
    let degree = usize::try_from(curve.degree()).ok()?;
    Some([
        *curve.knots().get(degree)?,
        *curve.knots().get(curve.pole_count())?,
    ])
}

/// The NURBS curve that equals `curve` over `range`.
pub(super) fn curve_nurbs(
    curve: &SolvedCurveGeometry,
    range: [f64; 2],
) -> Result<NurbsCurve, String> {
    let [lo, hi] = range;
    let frame_vectors = |frame: &cadmpeg_ir::units::OrthonormalFrame3| {
        (*frame.reference().as_raw(), *frame.binormal().as_raw())
    };
    let scaled = |vector: Vector3, scale: f64| {
        Vector3::new(vector.x * scale, vector.y * scale, vector.z * scale)
    };
    match curve {
        SolvedCurveGeometry::Line(line) => {
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            let at = |parameter: f64| {
                Point3::new(
                    origin.x + direction.x * parameter,
                    origin.y + direction.y * parameter,
                    origin.z + direction.z * parameter,
                )
            };
            exact::segment_curve(at(lo), at(hi), range)
        }
        SolvedCurveGeometry::Nurbs(nurbs) => exact::clamped_curve(nurbs, range),
        SolvedCurveGeometry::Circle(circle) => {
            let (r, s) = frame_vectors(circle.frame());
            let radius = circle.radius().get();
            exact::trigonometric_curve(
                circle.center().get(),
                scaled(r, radius),
                scaled(s, radius),
                false,
                range,
            )
            .map(|(curve, _)| curve)
        }
        SolvedCurveGeometry::Ellipse(ellipse) => {
            let (r, s) = frame_vectors(ellipse.frame());
            exact::trigonometric_curve(
                ellipse.center().get(),
                scaled(r, ellipse.major_radius().get()),
                scaled(s, ellipse.minor_radius().get()),
                false,
                range,
            )
            .map(|(curve, _)| curve)
        }
        SolvedCurveGeometry::Hyperbola(hyperbola) => {
            let (r, s) = frame_vectors(hyperbola.frame());
            exact::trigonometric_curve(
                hyperbola.center().get(),
                scaled(r, hyperbola.major_radius().get()),
                scaled(s, hyperbola.minor_radius().get()),
                true,
                range,
            )
            .map(|(curve, _)| curve)
        }
        SolvedCurveGeometry::Parabola(parabola) => {
            let (r, s) = frame_vectors(parabola.frame());
            let focal = parabola.focal_distance().get();
            exact::parabolic_curve(
                parabola.vertex().get(),
                scaled(r, focal),
                scaled(s, 2.0 * focal),
                range,
            )
        }
        SolvedCurveGeometry::Transformed(placed) => {
            let mut nurbs = curve_nurbs(placed.basis(), range)?;
            let transform = *placed.transform();
            nurbs
                .try_map_control_points(|_, pole| {
                    transform
                        .apply_point(pole.get())
                        .ok_or_else(|| "the placed curve leaves the finite range".to_owned())
                })
                .map_err(|error| error.clone())?;
            Ok(nurbs)
        }
        _ => Err("its curve kind has no NURBS form the writer builds".into()),
    }
}

/// The curves and compound constructions of the document. A joined curve
/// (a compound construction or a composite carrier) names its components
/// here.
pub(super) struct CompoundIndex<'a> {
    curves: HashMap<&'a str, &'a Curve>,
    compounds: HashMap<&'a str, &'a CompoundCurveConstruction>,
}

/// A joined curve: its components in order, each with whether it keeps its
/// own parameter direction.
enum Joint<'a> {
    Compound(&'a CompoundCurveConstruction),
    Composite(&'a CompositeCurveSegments),
}

impl<'a> Joint<'a> {
    fn components(&self) -> Vec<(&'a str, bool)> {
        match self {
            Self::Compound(compound) => compound
                .components()
                .iter()
                .map(|component| (component.component.as_str(), true))
                .collect(),
            Self::Composite(segments) => segments
                .iter()
                .map(|segment| (segment.curve.as_str(), segment.same_sense))
                .collect(),
        }
    }
}

impl<'a> CompoundIndex<'a> {
    pub(super) fn new(ir: &'a CadIr) -> Self {
        Self {
            curves: ir
                .geometry
                .curves
                .iter()
                .map(|curve| (curve.id.as_str(), curve))
                .collect(),
            compounds: ir
                .geometry
                .procedural_curves
                .iter()
                .filter_map(|procedure| match procedure.definition() {
                    ProceduralCurveDefinition::Compound(compound) => {
                        Some((procedure.id.as_str(), compound))
                    }
                    _ => None,
                })
                .collect(),
        }
    }

    /// The joint of `curve`: its compound construction where its cache holds
    /// no other solved form, else its composite carrier.
    fn joint(&self, curve: &'a Curve) -> Option<Joint<'a>> {
        if let CurveGeometry::Procedural {
            construction,
            cache:
                None | Some(SolvedCurveGeometry::Unknown { .. } | SolvedCurveGeometry::Composite { .. }),
        } = &curve.geometry
        {
            if let Some(compound) = self.compounds.get(construction.as_str()) {
                return Some(Joint::Compound(compound));
            }
        }
        match solved_curve(&curve.geometry) {
            Some(SolvedCurveGeometry::Composite { segments, .. }) => {
                Some(Joint::Composite(segments))
            }
            _ => None,
        }
    }

    /// Whether `curve` is a joined curve.
    pub(super) fn is_joined(&self, id: &str) -> bool {
        self.curves
            .get(id)
            .is_some_and(|curve| self.joint(curve).is_some())
    }

    /// The components of the joined curve `id` that are joined curves
    /// themselves.
    pub(super) fn joined_components(&self, id: &str) -> Vec<&'a str> {
        self.curves
            .get(id)
            .and_then(|curve| self.joint(curve))
            .map(|joint| joint.components())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(component, _)| {
                let (id, _) = self.curves.get_key_value(component)?;
                self.is_joined(id).then_some(*id)
            })
            .collect()
    }

    /// The components of each joined curve among `curves`, through nested
    /// joints. The joined curve holds them.
    pub(super) fn components<'b>(
        &self,
        curves: impl IntoIterator<Item = &'b str>,
    ) -> BTreeSet<&'a str> {
        let mut pending = curves
            .into_iter()
            .filter_map(|id| self.curves.get(id).copied())
            .collect::<Vec<_>>();
        let mut components = BTreeSet::new();
        while let Some(curve) = pending.pop() {
            for (component, _) in self
                .joint(curve)
                .map(|joint| joint.components())
                .unwrap_or_default()
            {
                if let Some((id, curve)) = self.curves.get_key_value(component) {
                    if components.insert(*id) {
                        pending.push(curve);
                    }
                }
            }
        }
        components
    }

    /// The breaks of `joint`: component `i` runs over
    /// `[breaks[i], breaks[i + 1]]`. A compound states them. A composite
    /// lays its components end to end, each as long as its own domain, from
    /// the start of `curve`'s range, else from the start of its first
    /// component's domain. A break within `BREAK_SLACK` of the next
    /// component's domain start, or of the end of `curve`'s range, takes that
    /// stored value, and a component whose domain starts at its break ends at
    /// its own stored domain end.
    fn breaks(
        &self,
        ctx: &DecodeContext<'_>,
        curve: &Curve,
        joint: &Joint<'_>,
    ) -> Result<Vec<f64>, String> {
        match joint {
            Joint::Compound(compound) => Ok(compound
                .parameters()
                .iter()
                .map(|parameter| parameter.get())
                .collect()),
            Joint::Composite(_) => {
                let _nested = ctx
                    .enter_nested("Rhino writer composite curve")
                    .map_err(|error| error.to_string())?;
                let mut breaks: Vec<f64> = Vec::new();
                for (id, _) in joint.components() {
                    let component = self
                        .curves
                        .get(id)
                        .ok_or_else(|| format!("its component {id} is missing"))?;
                    let [d0, d1] = self.domain(ctx, component)?;
                    let start = match breaks.last_mut() {
                        Some(last) => {
                            if within_break_slack(*last, d0) {
                                *last = d0;
                            }
                            *last
                        }
                        None => {
                            let start = curve
                                .parameter_range
                                .map_or(d0, |range| range.endpoints()[0]);
                            breaks.push(start);
                            start
                        }
                    };
                    breaks.push(if d0 == start { d1 } else { start + (d1 - d0) });
                }
                if let (Some(range), Some(last)) = (curve.parameter_range, breaks.last_mut()) {
                    let end = range.endpoints()[1];
                    if within_break_slack(*last, end) {
                        *last = end;
                    }
                }
                Ok(breaks)
            }
        }
    }

    /// The parameter interval of `curve`.
    pub(super) fn domain(
        &self,
        ctx: &DecodeContext<'_>,
        curve: &Curve,
    ) -> Result<[f64; 2], String> {
        if let Some(range) = curve.parameter_range {
            return Ok(range.endpoints());
        }
        if let Some(joint) = self.joint(curve) {
            let breaks = self.breaks(ctx, curve, &joint)?;
            if let (Some(first), Some(last)) = (breaks.first(), breaks.last()) {
                return Ok([*first, *last]);
            }
        }
        match solved_curve(&curve.geometry) {
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => knot_domain(nurbs),
            Some(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)) => {
                Some([0.0, std::f64::consts::TAU])
            }
            _ => None,
        }
        .ok_or_else(|| {
            format!(
                "component {} states no parameter interval",
                curve.id.as_str()
            )
        })
    }

    /// The NURBS curve that equals the joined curve `id` over `range`, where
    /// `id` is a joined curve.
    pub(super) fn joined_nurbs(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        range: [f64; 2],
    ) -> Option<Result<NurbsCurve, String>> {
        let curve = self.curves.get(id)?;
        let joint = self.joint(curve)?;
        Some(
            self.nurbs(ctx, curve, &joint, range, 0.0)
                .map(|(nurbs, _)| nurbs),
        )
    }

    /// The NURBS curve that equals `curve`, joined by `joint`, over `range`:
    /// each component over its part of `range`, joined end to end, with the
    /// widest gap past `JOIN_GAP` it closes. The join moves the meeting ends
    /// of two components to their midpoint, and ends farther apart than both
    /// `JOIN_GAP` and `gap_limit` are an error.
    fn nurbs(
        &self,
        ctx: &DecodeContext<'_>,
        curve: &Curve,
        joint: &Joint<'_>,
        range: [f64; 2],
        gap_limit: f64,
    ) -> Result<(NurbsCurve, f64), String> {
        let _nested = ctx
            .enter_nested("Rhino writer compound curve")
            .map_err(|error| error.to_string())?;
        let pieces = self.pieces(ctx, curve, joint, range, gap_limit)?;
        let mut widest = pieces.widest;
        for pair in pieces.curves.windows(2) {
            let gap = meeting_gap(&pair[0].0, &pair[1].0)?;
            if gap.closed {
                continue;
            }
            if gap.distance <= gap_limit {
                widest = widest.max(gap.distance);
                continue;
            }
            return Err(format!(
                "the components are {} apart where they meet",
                gap.distance
            ));
        }
        crate::curves::join_nurbs_segments(
            ctx,
            pieces.curves.into_iter().map(|(curve, _)| curve).collect(),
            0,
        )
        .map(|join| (join.curve, widest))
        .map_err(|error| format!("the components do not join into one NURBS curve: {error}"))
    }

    /// Each component of `curve`, joined by `joint`, as a NURBS curve over
    /// its part of `range`, with that part, in order along `curve`, and the
    /// widest gap past `JOIN_GAP` that the joins of nested components close.
    fn pieces(
        &self,
        ctx: &DecodeContext<'_>,
        curve: &Curve,
        joint: &Joint<'_>,
        range: [f64; 2],
        gap_limit: f64,
    ) -> Result<Pieces, String> {
        let parameters = self.breaks(ctx, curve, joint)?;
        let components = joint.components();
        if parameters.len() != components.len() + 1 {
            return Err("the compound states no interval for each component".into());
        }
        if parameters.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("the compound's component intervals are out of order".into());
        }
        let (first, last) = (parameters[0], parameters[components.len()]);
        let lo = if within_break_slack(first, range[0]) {
            first
        } else {
            range[0]
        };
        let hi = if within_break_slack(last, range[1]) {
            last
        } else {
            range[1]
        };
        if lo < first || hi > last {
            return Err(format!(
                "the range [{lo}, {hi}] leaves the compound's interval [{first}, {last}]"
            ));
        }
        let mut pieces = Vec::new();
        let mut widest = 0.0_f64;
        for (index, (id, same_sense)) in components.into_iter().enumerate() {
            let [start, end] = [parameters[index], parameters[index + 1]];
            let span = [lo.max(start), hi.min(end)];
            if span[0] >= span[1] {
                continue;
            }
            let component = self
                .curves
                .get(id)
                .ok_or_else(|| format!("its component {id} is missing"))?;
            let [d0, d1] = self.domain(ctx, component)?;
            let local = span.map(|parameter| {
                let (near, far) = if same_sense {
                    (start, end)
                } else {
                    (end, start)
                };
                if parameter == near {
                    d0
                } else if parameter == far {
                    d1
                } else {
                    let fraction = (parameter - start) / (end - start);
                    let fraction = if same_sense { fraction } else { 1.0 - fraction };
                    d0 + fraction * (d1 - d0)
                }
            });
            let local = if same_sense {
                local
            } else {
                [local[1], local[0]]
            };
            let mut piece = match self.joint(component) {
                Some(nested) => {
                    let (nested, gap) = self.nurbs(ctx, component, &nested, local, gap_limit)?;
                    widest = widest.max(gap);
                    nested
                }
                None => match solved_curve(&component.geometry) {
                    None | Some(SolvedCurveGeometry::Unknown { .. }) => {
                        return Err(format!("its component {id} has no solved form"));
                    }
                    Some(solved) => curve_nurbs(solved, local)
                        .map_err(|reason| format!("component {id}: {reason}"))?,
                },
            };
            if !same_sense {
                piece.reverse_parameterization();
            }
            let target = span
                .map(FiniteReal::new)
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .and_then(|target| <[FiniteReal; 2]>::try_from(target).ok())
                .ok_or("a component interval leaves the finite range")?;
            pieces.push((
                crate::curves::remap_nurbs_domain(piece, target, 0)
                    .map_err(|error| format!("component {id}: {error}"))?,
                span,
            ));
        }
        Ok(Pieces {
            curves: pieces,
            widest,
        })
    }
}

/// The components of a joined curve over a range, each with its interval
/// of the joined curve's parameter, and the widest gap that the joins of
/// nested components close.
struct Pieces {
    curves: Vec<(NurbsCurve, [f64; 2])>,
    widest: f64,
}

/// Where the end of one curve piece meets the start of the next.
struct MeetingGap {
    /// The distance between the two ends.
    distance: f64,
    /// The two ends lie within `JOIN_GAP`, relative to their size.
    closed: bool,
    /// The midpoint of the two ends.
    midpoint: Point3,
    /// The farther of the two ends from the midpoint.
    reach: f64,
}

fn meeting_gap(before: &NurbsCurve, after: &NurbsCurve) -> Result<MeetingGap, String> {
    let end = before
        .pole_rows()
        .point_at(before.pole_count() - 1)
        .map(cadmpeg_ir::features::geometry::FinitePoint3::get);
    let start = after
        .pole_rows()
        .point_at(0)
        .map(cadmpeg_ir::features::geometry::FinitePoint3::get);
    let (Some(end), Some(start)) = (end, start) else {
        return Err("a component has no control point".into());
    };
    let scale = [end.x, end.y, end.z]
        .iter()
        .fold(1.0_f64, |scale, lane| scale.max(lane.abs()));
    let distance = end.distance(start);
    let midpoint = Point3::new(
        f64::midpoint(end.x, start.x),
        f64::midpoint(end.y, start.y),
        f64::midpoint(end.z, start.z),
    );
    Ok(MeetingGap {
        distance,
        closed: distance <= exact::JOIN_GAP * scale,
        midpoint,
        reach: midpoint.distance(end).max(midpoint.distance(start)),
    })
}

/// Relative distance within which a composite break laid by component
/// lengths meets a stored parameter.
const BREAK_SLACK: f64 = 1e-9;

fn within_break_slack(laid: f64, stored: f64) -> bool {
    (laid - stored).abs() <= BREAK_SLACK * laid.abs().max(stored.abs()).max(1.0)
}

/// The edge curve in 3DM form where it needs another one, with the widest
/// gap it closes between joined pieces that meet within `tolerance`, `None`
/// where the stated solved carrier is written as it is.
fn edge_curve(
    ctx: &DecodeContext<'_>,
    compounds: &CompoundIndex<'_>,
    edge: &Edge,
    curve: &Curve,
    tolerance: f64,
) -> Result<Option<(SolvedCurveGeometry, f64)>, String> {
    let range = edge
        .param_range()
        .ok_or("the edge states no parameter range")?
        .get();
    if range[0] == range[1] {
        return Err("the edge has a zero-length parameter range".into());
    }
    if let Some(joint) = compounds.joint(curve) {
        return compounds
            .nurbs(ctx, curve, &joint, range, tolerance)
            .and_then(|(nurbs, gap)| Ok((over_range(nurbs, range)?, gap)))
            .map(|(nurbs, gap)| Some((SolvedCurveGeometry::Nurbs(nurbs), gap)))
            .map_err(|reason| format!("curve {}: {reason}", curve.id.as_str()));
    }
    let solved = solved_curve(&curve.geometry)
        .ok_or_else(|| format!("curve {} has no solved form", curve.id.as_str()))?;
    let procedural = !matches!(curve.geometry, CurveGeometry::Solved(_));
    let writable = match solved {
        SolvedCurveGeometry::Line(line) => {
            (line.direction().as_raw().norm() - 1.0).abs() <= EPS_WRITE_DEGENERATE
        }
        SolvedCurveGeometry::Nurbs(nurbs) => !nurbs.periodic() && knot_domain(nurbs) == Some(range),
        _ => false,
    };
    if writable {
        return Ok(procedural.then(|| (solved.clone(), 0.0)));
    }
    curve_nurbs(solved, range)
        .map(|nurbs| Some((SolvedCurveGeometry::Nurbs(nurbs), 0.0)))
        .map_err(|reason| format!("curve {}: {reason}", curve.id.as_str()))
}

/// `nurbs` with its knot domain mapped onto `range`: a joined curve takes
/// the stored breaks of its components at ends within `BREAK_SLACK` of the
/// range, and a 3DM edge curve spans its edge's range.
fn over_range(nurbs: NurbsCurve, range: [f64; 2]) -> Result<NurbsCurve, String> {
    if knot_domain(&nurbs) == Some(range) {
        return Ok(nurbs);
    }
    let [Some(start), Some(end)] = range.map(FiniteReal::new) else {
        return Err("the edge range is not finite".to_owned());
    };
    crate::curves::remap_nurbs_domain(nurbs, [start, end], 0).map_err(|error| error.to_string())
}

/// The pcurve uses of `coedge` that lie on `surface`: each use whose pcurve
/// states no other support surface.
fn face_uses<'a>(
    coedge: &'a Coedge,
    surface: &str,
    pcurves: &HashMap<&str, &'a Pcurve>,
) -> Vec<(&'a cadmpeg_ir::topology::PcurveUse, &'a Pcurve)> {
    coedge
        .pcurves
        .iter()
        .filter_map(|use_| Some((use_, *pcurves.get(use_.pcurve.as_str())?)))
        .filter(|(_, pcurve)| match &pcurve.metadata {
            PcurveMetadata::General { form } => form
                .support_surface
                .as_ref()
                .is_none_or(|support| support.as_str() == surface),
            PcurveMetadata::AsmInline { .. } => true,
        })
        .collect()
}

/// The interval of `pcurve` that `use_` runs over, in the coedge's direction.
pub(super) fn use_interval(
    use_: &cadmpeg_ir::topology::PcurveUse,
    pcurve: &Pcurve,
    edge_range: Option<[f64; 2]>,
) -> Result<[f64; 2], String> {
    if let Some(range) = use_.parameter_range {
        return Ok(range.endpoints());
    }
    if let Some(range) = pcurve.parameter_range() {
        return Ok(range.get());
    }
    match &pcurve.geometry {
        PcurveGeometry::Nurbs { nurbs } => {
            let degree = usize::try_from(nurbs.degree()).map_err(|_| "the degree overflows")?;
            let knots = nurbs.knots();
            match (knots.get(degree), knots.get(nurbs.pole_rows().count())) {
                (Some(lo), Some(hi)) => Ok([*lo, *hi]),
                _ => Err("the pcurve has no knot domain".into()),
            }
        }
        _ => edge_range.ok_or_else(|| "the pcurve and its edge state no interval".into()),
    }
}

/// The knot domain of a NURBS pcurve.
fn pcurve_knot_domain(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        return None;
    };
    let degree = usize::try_from(nurbs.degree()).ok()?;
    Some([
        *nurbs.knots().get(degree)?,
        *nurbs.knots().get(nurbs.pole_rows().count())?,
    ])
}

/// `interval` with its ends in increasing order.
fn increasing(interval: [f64; 2]) -> [f64; 2] {
    [interval[0].min(interval[1]), interval[0].max(interval[1])]
}

/// Whether `pcurve`, used by `use_` on an unchanged plane or NURBS face, is
/// written as it is: the form the Brep writer admits directly.
fn written_as_is(
    use_: &cadmpeg_ir::topology::PcurveUse,
    pcurve: &Pcurve,
    edge_range: Option<[f64; 2]>,
) -> bool {
    if pcurve.native_tail_flags().is_some() {
        return false;
    }
    let stated = pcurve
        .parameter_range()
        .map(cadmpeg_ir::units::FiniteVector::get);
    let Ok(interval) = use_interval(use_, pcurve, edge_range).map(increasing) else {
        return false;
    };
    if interval[0] >= interval[1] || stated.is_some_and(|stated| stated != interval) {
        return false;
    }
    match &pcurve.geometry {
        PcurveGeometry::Line(_) => true,
        PcurveGeometry::Nurbs { nurbs } => {
            !nurbs.periodic()
                && usize::try_from(nurbs.degree()).is_ok_and(|degree| {
                    let knots = nurbs.knots();
                    knots.get(degree) == Some(&interval[0])
                        && knots.get(nurbs.pole_rows().count()) == Some(&interval[1])
                })
        }
        _ => false,
    }
}

/// How one face's surface is written.
enum FaceSurface {
    /// The stated solved plane or nonperiodic NURBS patch, as it is.
    AsIs,
    /// A plane or a NURBS patch whose parameters are the stated ones.
    Same(SolvedSurfaceGeometry),
    /// A NURBS patch whose parameters the maps give from the stated ones.
    Mapped(Box<SolvedSurfaceGeometry>, ParameterMap, ParameterMap),
}

/// The parameter box of `splines`: the hull of their control points, which
/// holds each spline.
fn parameter_hull(splines: &[Spline]) -> Option<([f64; 2], [f64; 2])> {
    let mut u = [f64::INFINITY, f64::NEG_INFINITY];
    let mut v = [f64::INFINITY, f64::NEG_INFINITY];
    for point in splines.iter().flat_map(Spline::points) {
        u = [u[0].min(point.x), u[1].max(point.x)];
        v = [v[0].min(point.y), v[1].max(point.y)];
    }
    (u[0] < u[1] && v[0] < v[1]).then_some((u, v))
}

pub(super) fn nurbs_surface_domain(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
) -> Option<([f64; 2], [f64; 2])> {
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    Some((
        [
            *surface.u_knots().get(u_degree)?,
            *surface.u_knots().get(surface.u_count())?,
        ],
        [
            *surface.v_knots().get(v_degree)?,
            *surface.v_knots().get(surface.v_count())?,
        ],
    ))
}

/// The written form of `surface` for a face whose pcurves are `splines`.
fn face_surface(surface: &SurfaceGeometry, splines: &[Spline]) -> Result<FaceSurface, String> {
    let solved = solved_surface(surface).ok_or("its surface has no solved form")?;
    let procedural = !matches!(surface, SurfaceGeometry::Solved(_));
    let hull = || {
        parameter_hull(splines)
            .ok_or_else(|| "its pcurves span no parameter area of its surface".to_owned())
    };
    match solved {
        SolvedSurfaceGeometry::Plane(_) => Ok(if procedural {
            FaceSurface::Same(solved.clone())
        } else {
            FaceSurface::AsIs
        }),
        SolvedSurfaceGeometry::Nurbs(nurbs) if !nurbs.u_periodic() && !nurbs.v_periodic() => {
            Ok(if procedural {
                FaceSurface::Same(solved.clone())
            } else {
                FaceSurface::AsIs
            })
        }
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            let (u_domain, v_domain) =
                nurbs_surface_domain(nurbs).ok_or("its NURBS surface has no knot domain")?;
            let (u_hull, v_hull) = hull()?;
            let u = if nurbs.u_periodic() { u_hull } else { u_domain };
            let v = if nurbs.v_periodic() { v_hull } else { v_domain };
            exact::clamped_surface(nurbs, u, v)
                .map(|patch| FaceSurface::Same(SolvedSurfaceGeometry::Nurbs(patch)))
        }
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_) => {
            let (u, v) = hull()?;
            let patch = exact::analytic_patch(solved, u, v)?;
            Ok(FaceSurface::Mapped(
                Box::new(SolvedSurfaceGeometry::Nurbs(patch.surface)),
                patch.u,
                patch.v,
            ))
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            let basis = match placed.basis() {
                SolvedSurfaceGeometry::Plane(plane) => {
                    let (u, v) = hull()?;
                    FaceSurface::Same(SolvedSurfaceGeometry::Nurbs(exact::plane_patch(
                        plane.origin().get(),
                        *plane.frame().reference().as_raw(),
                        *plane.frame().binormal().as_raw(),
                        u,
                        v,
                    )?))
                }
                basis => match face_surface(&SurfaceGeometry::Solved(basis.clone()), splines)? {
                    FaceSurface::AsIs => FaceSurface::Same(basis.clone()),
                    other => other,
                },
            };
            let transform = *placed.transform();
            let place = |geometry: SolvedSurfaceGeometry| match geometry {
                SolvedSurfaceGeometry::Nurbs(mut nurbs) => {
                    nurbs
                        .try_map_control_points(|_, pole| {
                            transform.apply_point(pole.get()).ok_or_else(|| {
                                "the placed surface leaves the finite range".to_owned()
                            })
                        })
                        .map_err(|error| error.clone())?;
                    Ok(SolvedSurfaceGeometry::Nurbs(nurbs))
                }
                _ => Err("its placed surface has no NURBS form the writer builds".to_owned()),
            };
            match basis {
                FaceSurface::AsIs => Err("its placed surface has no NURBS form".into()),
                FaceSurface::Same(geometry) => Ok(FaceSurface::Same(place(geometry)?)),
                FaceSurface::Mapped(geometry, u, v) => {
                    Ok(FaceSurface::Mapped(Box::new(place(*geometry)?), u, v))
                }
            }
        }
        _ => Err("its surface kind has no NURBS form the writer builds".into()),
    }
}

/// One coedge's pcurve as the face writes it.
struct CoedgePcurve {
    /// The pcurve record the coedge keeps.
    id: String,
    /// The written pcurve in the stated surface parameters, `None` where the
    /// record is written as it is.
    spline: Option<Spline>,
    /// Whether the coedge runs `spline` backwards.
    against: bool,
}

/// How the face's surface takes its pcurves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PcurveRule {
    /// A plane: a pcurve is kept where it is written as it is over its
    /// edge's range, and the trim is otherwise the edge's projection.
    Plane,
    /// A nonperiodic NURBS patch written as it is.
    AsIs,
    /// A surface written in another form: every pcurve is rebuilt.
    Rebuilt,
}

/// The pcurve of `coedge` on `face`, `None` where the trim is the projection
/// of the edge onto the face's plane.
fn coedge_pcurve(
    coedge: &Coedge,
    surface: &str,
    edge_range: Option<[f64; 2]>,
    pcurves: &HashMap<&str, &Pcurve>,
    rule: PcurveRule,
) -> Result<Option<CoedgePcurve>, String> {
    let uses = face_uses(coedge, surface, pcurves);
    if let [(use_, pcurve)] = uses.as_slice() {
        let as_is = written_as_is(use_, pcurve, edge_range);
        let kept = match rule {
            PcurveRule::Plane => {
                as_is && use_interval(use_, pcurve, edge_range).ok().map(increasing) == edge_range
            }
            PcurveRule::AsIs => as_is,
            PcurveRule::Rebuilt => false,
        };
        if kept {
            return Ok(Some(CoedgePcurve {
                id: pcurve.id.as_str().to_owned(),
                spline: None,
                against: false,
            }));
        }
    }
    let Some((_, first)) = uses.first() else {
        return Ok(None);
    };
    if rule == PcurveRule::Plane {
        return Ok(None);
    }
    let mut intervals = Vec::with_capacity(uses.len());
    for (use_, pcurve) in &uses {
        let interval = use_interval(use_, pcurve, edge_range)
            .map_err(|reason| format!("pcurve {}: {reason}", pcurve.id.as_str()))?;
        intervals.push((interval, *pcurve));
    }
    let against = intervals
        .first()
        .is_some_and(|([start, end], _)| start > end);
    if intervals
        .iter()
        .any(|([start, end], _)| (start > end) != against)
    {
        return Err(format!(
            "the pcurves of coedge {} run in different directions",
            coedge.id.as_str()
        ));
    }
    if against {
        intervals.reverse();
    }
    let mut joined: Option<Spline> = None;
    for (interval, pcurve) in intervals {
        let spline = exact::pcurve_spline(&pcurve.geometry, interval)
            .map_err(|reason| format!("pcurve {}: {reason}", pcurve.id.as_str()))?;
        joined = Some(match joined {
            None => spline,
            Some(previous) => previous.join(spline).map_err(|reason| {
                format!(
                    "the pcurves of coedge {} do not join into one NURBS curve: {reason}",
                    coedge.id.as_str()
                )
            })?,
        });
    }
    Ok(Some(CoedgePcurve {
        id: first.id.as_str().to_owned(),
        spline: joined,
        against,
    }))
}

/// What one face becomes.
struct FacePlan {
    surface: FaceSurface,
    /// For each coedge of the face, its pcurve.
    pcurves: Vec<(String, Option<CoedgePcurve>)>,
}

/// Why an edge is written as one edge for each of its uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Unjoin {
    /// Its two uses run the same way.
    EqualSenses,
    /// Its two uses are not each other's radial next.
    UnpairedRing,
    /// It has three or more uses.
    ManyUses,
}

impl Unjoin {
    /// The reason clause of the writer's loss.
    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::EqualSenses => "their two uses run the same way",
            Self::UnpairedRing => "their two uses are not each other's radial next",
            Self::ManyUses => "each has three or more uses, and a written edge joins two",
        }
    }
}

/// Gives each use of an edge its own edge unless the edge has one use, or two
/// uses that run opposite ways and are each other's radial next: a 3DM Brep
/// joins two faces at an edge where their trims run opposite ways. Each use
/// after the first takes a copy of the edge, with the key of the use appended
/// to its id. Returns the id of each edge it splits, with the reason.
fn unjoin_unpaired_uses(ir: &mut CadIr) -> Vec<(Unjoin, String)> {
    let geometry = &mut ir.geometry;
    let mut uses = BTreeMap::<String, Vec<usize>>::new();
    for (index, coedge) in geometry.coedges.iter().enumerate() {
        uses.entry(coedge.edge.as_str().to_owned())
            .or_default()
            .push(index);
    }
    let mut unjoined = Vec::new();
    let mut copies = Vec::new();
    for (edge, uses) in uses {
        let reason = match uses[..] {
            [] | [_] => continue,
            [first, second] => {
                let (one, other) = (&geometry.coedges[first], &geometry.coedges[second]);
                if one.radial_next != other.id || other.radial_next != one.id {
                    Unjoin::UnpairedRing
                } else if one.sense == other.sense {
                    Unjoin::EqualSenses
                } else {
                    continue;
                }
            }
            _ => Unjoin::ManyUses,
        };
        let Some(source) = geometry
            .edges
            .iter()
            .find(|source| source.id.as_str() == edge)
        else {
            continue;
        };
        let Some(ids) = uses[1..]
            .iter()
            .map(|&index| {
                geometry.coedges[index]
                    .id
                    .as_str()
                    .split_once('#')
                    .and_then(|(_, key)| {
                        cadmpeg_ir::ids::EdgeId::mint(format!("{edge}.on-{key}")).ok()
                    })
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        for (&index, id) in uses[1..].iter().zip(ids) {
            let mut copy = source.clone();
            copy.id = id.clone();
            copies.push(copy);
            geometry.coedges[index].edge = id;
        }
        for &index in &uses {
            let coedge = &mut geometry.coedges[index];
            coedge.radial_next = coedge.id.clone();
        }
        unjoined.push((reason, edge));
    }
    geometry.edges.extend(copies);
    unjoined
}

/// Gives each face its own surface record and each edge its own curve
/// record, as a 3DM Brep holds them. A copy's id is its source's id with the
/// key of the face or edge that uses it appended. Returns the source id of
/// each copy.
fn unshare_carriers(ir: &mut CadIr) -> HashMap<String, String> {
    let geometry = &mut ir.geometry;
    let key = |id: &str| id.split_once('#').map(|(_, key)| key.to_owned());
    let mut origins = HashMap::new();
    let mut seen = BTreeSet::new();
    let mut surfaces = Vec::new();
    for face in &mut geometry.faces {
        if seen.insert(face.surface.as_str().to_owned()) {
            continue;
        }
        let copy = key(face.id.as_str())
            .and_then(|key| SurfaceId::mint(format!("{}.on-{key}", face.surface.as_str())).ok())
            .zip(
                geometry
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == face.surface),
            );
        if let Some((id, source)) = copy {
            let mut copy = source.clone();
            copy.id = id.clone();
            origins.insert(id.as_str().to_owned(), face.surface.as_str().to_owned());
            surfaces.push(copy);
            face.surface = id;
        }
    }
    geometry.surfaces.extend(surfaces);
    seen.clear();
    let mut curves = Vec::new();
    for edge in &mut geometry.edges {
        let Some(curve) = edge.curve().cloned() else {
            continue;
        };
        if seen.insert(curve.as_str().to_owned()) {
            continue;
        }
        let copy = key(edge.id.as_str())
            .and_then(|key| CurveId::mint(format!("{}.on-{key}", curve.as_str())).ok())
            .zip(geometry.curves.iter().find(|source| source.id == curve));
        if let Some((id, source)) = copy {
            let mut copy = source.clone();
            copy.id = id.clone();
            if edge.set_curve(Some(id)).is_ok() {
                curves.push(copy);
            }
        }
    }
    geometry.curves.extend(curves);
    origins
}

/// An edge written as several edges: its id, the ids of the edges it
/// becomes, the widest gap between their curves, and the widest tolerance
/// an edge takes at a gap.
pub(super) struct EdgeSplit {
    pub(super) edge: String,
    pub(super) pieces: Vec<String>,
    pub(super) gap: f64,
    pub(super) tolerance: f64,
}

/// Where the joined curve of an edge is cut.
struct EdgeCuts {
    /// The edge parameter range.
    range: [f64; 2],
    /// The edge parameters that bound each run of pieces, increasing.
    bounds: Vec<f64>,
    /// The gap at each cut, in the order the edge runs.
    gaps: Vec<MeetingGap>,
}

/// Where the pcurve uses of an edge are cut.
struct TrimCuts {
    /// For each coedge id and pcurve use index, the use's parameters in the
    /// order the edge runs: its start, each cut, and its end.
    parameters: BTreeMap<(String, usize), Vec<f64>>,
    /// For each cut, the farthest a piece end or a trim end at the cut can
    /// lie from a piece curve end there: the gap's reach plus the farthest a
    /// trim end lies from the vertex.
    reach: Vec<f64>,
}

/// The records that replace one edge.
struct SplitPlan {
    split: EdgeSplit,
    edge: String,
    edges: Vec<Edge>,
    curves: Vec<Curve>,
    points: Vec<cadmpeg_ir::topology::Point>,
    vertices: Vec<cadmpeg_ir::topology::Vertex>,
    /// The coedges that replace each coedge of the edge, by its id.
    coedges: BTreeMap<String, Vec<Coedge>>,
    pcurves: Vec<Pcurve>,
    loops: Vec<cadmpeg_ir::topology::Loop>,
}

/// `id` with its kind replaced by `kind` and `suffix` appended to its key.
fn derived_id(id: &str, kind: &str, suffix: &str) -> Option<String> {
    let (head, key) = id.split_once('#')?;
    let (scope, _) = head.rsplit_once(':')?;
    Some(format!("{scope}:{kind}#{key}.{suffix}"))
}

/// Writes each edge whose joined curve has pieces that meet farther apart
/// than the edge tolerance as one edge for each run of pieces that meet
/// within it. Each gap gets a vertex at the midpoint of its two ends, with a
/// tolerance that reaches both. Each trim of the edge breaks where its image
/// comes closest to that vertex, and the edges at the gap take a tolerance
/// that reaches the vertex and each trim end. Returns each edge it splits.
fn split_gapped_edges(
    ctx: &DecodeContext<'_>,
    compounds: &CompoundIndex<'_>,
    ir: &mut CadIr,
) -> Result<Vec<EdgeSplit>, ResourceLimit> {
    let edges = ir
        .geometry
        .edges
        .iter()
        .map(|edge| edge.id.as_str().to_owned())
        .collect::<Vec<_>>();
    let mut splits = Vec::new();
    for edge in edges {
        let Some(cuts) = edge_cuts(ctx, compounds, ir, &edge) else {
            continue;
        };
        let Some(trims) = trim_cuts(ctx, ir, &edge, &cuts)? else {
            continue;
        };
        let Some(plan) = plan_split(ir, &edge, &cuts, &trims) else {
            continue;
        };
        let geometry = &mut ir.geometry;
        for loop_ in plan.loops {
            if let Some(slot) = geometry.loops.iter_mut().find(|slot| slot.id == loop_.id) {
                *slot = loop_;
            }
        }
        let mut pieces = plan.coedges;
        geometry.coedges = std::mem::take(&mut geometry.coedges)
            .into_iter()
            .flat_map(|coedge| {
                pieces
                    .remove(coedge.id.as_str())
                    .unwrap_or_else(|| vec![coedge])
            })
            .collect();
        let Some(position) = geometry
            .edges
            .iter()
            .position(|source| source.id.as_str() == plan.edge)
        else {
            continue;
        };
        let curve = geometry.edges[position].curve().cloned();
        geometry.edges.splice(position..=position, plan.edges);
        if let Some(curve) = curve {
            if !geometry
                .edges
                .iter()
                .any(|source| source.curve() == Some(&curve))
            {
                geometry.curves.retain(|source| source.id != curve);
            }
        }
        geometry.curves.extend(plan.curves);
        geometry.points.extend(plan.points);
        geometry.vertices.extend(plan.vertices);
        geometry.pcurves.extend(plan.pcurves);
        splits.push(plan.split);
    }
    Ok(splits)
}

/// Where the joined curve of `edge` has pieces that meet farther apart than
/// the edge tolerance, `None` where the edge stays as it is.
fn edge_cuts(
    ctx: &DecodeContext<'_>,
    compounds: &CompoundIndex<'_>,
    ir: &CadIr,
    edge: &str,
) -> Option<EdgeCuts> {
    let geometry = &ir.geometry;
    let source = geometry
        .edges
        .iter()
        .find(|source| source.id.as_str() == edge)?;
    let curve = geometry
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == source.curve())?;
    let joint = compounds.joint(curve)?;
    let range = source.param_range()?.get();
    let tolerance = super::stated_write_tolerance(source.tolerance, ir);
    let pieces = compounds
        .pieces(ctx, curve, &joint, range, tolerance)
        .ok()?
        .curves;
    let mut bounds = vec![pieces.first()?.1[0]];
    let mut gaps = Vec::new();
    for pair in pieces.windows(2) {
        let gap = meeting_gap(&pair[0].0, &pair[1].0).ok()?;
        if !gap.closed && gap.distance > tolerance {
            bounds.push(pair[0].1[1]);
            gaps.push(gap);
        }
    }
    if gaps.is_empty() {
        return None;
    }
    bounds.push(pieces.last()?.1[1]);
    Some(EdgeCuts {
        range,
        bounds,
        gaps,
    })
}

/// Where each pcurve use of `edge` is cut: at each gap of `cuts`, the
/// parameter after the previous cut where the image of the pcurve through
/// its surface comes closest to the gap's vertex. `None` where a pcurve or
/// its surface is missing or has no finite image there.
fn trim_cuts(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    edge: &str,
    cuts: &EdgeCuts,
) -> Result<Option<TrimCuts>, ResourceLimit> {
    use cadmpeg_ir::topology::Sense;

    let geometry = &ir.geometry;
    let mut parameters = BTreeMap::new();
    let mut apart = vec![0.0_f64; cuts.gaps.len()];
    for coedge in geometry
        .coedges
        .iter()
        .filter(|coedge| coedge.edge.as_str() == edge)
    {
        let Some(face_surface) = geometry
            .loops
            .iter()
            .find(|loop_| loop_.id == coedge.owner_loop)
            .and_then(|loop_| geometry.faces.iter().find(|face| face.id == loop_.face))
            .map(|face| &face.surface)
        else {
            return Ok(None);
        };
        for (index, use_) in coedge.pcurves.iter().enumerate() {
            let Some(pcurve) = geometry
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == use_.pcurve)
            else {
                return Ok(None);
            };
            let support = match &pcurve.metadata {
                PcurveMetadata::General { form } => {
                    form.support_surface.as_ref().unwrap_or(face_surface)
                }
                PcurveMetadata::AsmInline { .. } => face_surface,
            };
            let Some(surface) = geometry
                .surfaces
                .iter()
                .find(|surface| &surface.id == support)
            else {
                return Ok(None);
            };
            let Ok(interval) = use_interval(use_, pcurve, Some(cuts.range)) else {
                return Ok(None);
            };
            let along = if coedge.sense == Sense::Forward {
                interval
            } else {
                [interval[1], interval[0]]
            };
            let knots = match &pcurve.geometry {
                PcurveGeometry::Nurbs { nurbs } => nurbs.knots().to_vec(),
                _ => Vec::new(),
            };
            let mut cut = vec![along[0]];
            for (number, gap) in cuts.gaps.iter().enumerate() {
                let from = cut[cut.len() - 1];
                let samples = super::span_samples(increasing([from, along[1]]), knots.clone());
                let distance = |parameter: f64| {
                    image_distance(
                        ctx,
                        &pcurve.geometry,
                        &surface.geometry,
                        gap.midpoint,
                        parameter,
                    )
                };
                let distances = samples
                    .iter()
                    .map(|parameter| distance(*parameter))
                    .collect::<Result<Vec<_>, _>>()?;
                let Some((parameter, distance)) =
                    super::refine_closest(&samples, &distances, distance)?
                else {
                    return Ok(None);
                };
                if !distance.is_finite() {
                    return Ok(None);
                }
                apart[number] = apart[number].max(distance);
                cut.push(parameter);
            }
            cut.push(along[1]);
            parameters.insert((coedge.id.as_str().to_owned(), index), cut);
        }
    }
    let reach = cuts
        .gaps
        .iter()
        .zip(apart)
        .map(|(gap, apart)| gap.reach + apart)
        .collect();
    Ok(Some(TrimCuts { parameters, reach }))
}

/// The distance from `target` to the image of `pcurve` at `parameter`
/// through `surface`, infinite where the image has no finite point.
fn image_distance(
    ctx: &DecodeContext<'_>,
    pcurve: &PcurveGeometry,
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    target: Point3,
    parameter: f64,
) -> Result<f64, ResourceLimit> {
    use cadmpeg_ir::eval::EvaluationFailure;

    let uv = match cadmpeg_ir::eval::decode::pcurve_uv(ctx, pcurve, parameter) {
        Ok(uv) => uv.get(),
        Err(EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
        Err(
            EvaluationFailure::NoValue
            | EvaluationFailure::NonFinite(_)
            | EvaluationFailure::WorkExhausted,
        ) => return Ok(f64::INFINITY),
    };
    match cadmpeg_ir::eval::decode::surface_point(ctx, surface, uv.u, uv.v) {
        Ok(point) => Ok(point.get().distance(target)),
        Err(EvaluationFailure::ResourceLimit(limit)) => Err(limit),
        Err(
            EvaluationFailure::NoValue
            | EvaluationFailure::NonFinite(_)
            | EvaluationFailure::WorkExhausted,
        ) => Ok(f64::INFINITY),
    }
}

/// The records that replace `edge`, cut at `cuts` with its trims cut at
/// `trims`, `None` where the edge stays as it is.
fn plan_split(ir: &CadIr, edge: &str, cuts: &EdgeCuts, trims: &TrimCuts) -> Option<SplitPlan> {
    use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, PcurveId, PointId, VertexId};
    use cadmpeg_ir::scalar::PositiveLength;
    use cadmpeg_ir::topology::{EdgeEnds, Point, Sense, Vertex};

    let geometry = &ir.geometry;
    let source = geometry
        .edges
        .iter()
        .find(|source| source.id.as_str() == edge)?;
    let curve = geometry
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == source.curve())?;
    let ends = source.ends.as_ref()?;
    let tolerance = super::stated_write_tolerance(source.tolerance, ir);
    let bounds = &cuts.bounds;
    let count = bounds.len() - 1;

    let piece_ids = (0..count)
        .map(|run| EdgeId::mint(format!("{edge}.piece-{run}")).ok())
        .collect::<Option<Vec<_>>>()?;
    let mut points = Vec::new();
    let mut vertices = Vec::new();
    let mut chain = vec![ends.start.clone()];
    for (number, gap) in cuts.gaps.iter().enumerate() {
        let suffix = format!("break-{number}");
        let point = PointId::mint(derived_id(edge, "point", &suffix)?).ok()?;
        let vertex = VertexId::mint(derived_id(edge, "vertex", &suffix)?).ok()?;
        let reach = PositiveLength::new(gap.reach.next_up())?;
        points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(gap.midpoint)?,
            None,
        ));
        vertices.push(Vertex {
            id: vertex.clone(),
            point,
            tolerance: Some(reach),
            native_ref: None,
        });
        chain.push(vertex);
    }
    chain.push(ends.end.clone());

    let mut curves = Vec::new();
    let mut edges = Vec::new();
    for (run, id) in piece_ids.iter().enumerate() {
        let mut copy = curve.clone();
        copy.id = CurveId::mint(format!("{}.piece-{run}", curve.id.as_str())).ok()?;
        let mut piece = source.clone();
        piece.id = id.clone();
        piece.carrier = cadmpeg_ir::topology::EdgeCarrier::new(
            Some(copy.id.clone()),
            Some([bounds[run], bounds[run + 1]]),
        )
        .ok()?;
        piece.ends = Some(EdgeEnds::new(chain[run].clone(), chain[run + 1].clone()));
        let widest = [run.checked_sub(1), (run + 1 < count).then_some(run)]
            .into_iter()
            .flatten()
            .map(|cut| trims.reach[cut])
            .fold(tolerance, f64::max);
        if widest > tolerance {
            piece.tolerance = Some(PositiveLength::new(widest.next_up())?);
        }
        piece.native_ref = None;
        curves.push(copy);
        edges.push(piece);
    }

    let used = geometry
        .coedges
        .iter()
        .filter(|coedge| coedge.edge.as_str() == edge)
        .collect::<Vec<_>>();
    let piece_coedge =
        |id: &CoedgeId, run: usize| CoedgeId::mint(format!("{}.piece-{run}", id.as_str())).ok();
    let mut coedges = BTreeMap::new();
    let mut pcurves = Vec::new();
    let mut rings = BTreeMap::<&str, Vec<CoedgeId>>::new();
    for coedge in &used {
        if !used.iter().any(|other| other.id == coedge.radial_next) {
            return None;
        }
        let forward = coedge.sense == Sense::Forward;
        let mut uses = vec![Vec::new(); piece_ids.len()];
        for (index, use_) in coedge.pcurves.iter().enumerate() {
            let pcurve = geometry
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == use_.pcurve)?;
            let parameters = trims
                .parameters
                .get(&(coedge.id.as_str().to_owned(), index))?;
            let along = [parameters[0], parameters[parameters.len() - 1]];
            for run in 0..count {
                let [start, end] = [parameters[run], parameters[run + 1]];
                if (end - start) * (along[1] - along[0]) <= 0.0 {
                    return None;
                }
                let directed = if forward { [start, end] } else { [end, start] };
                let id = PcurveId::mint(format!("{}.piece-{run}", pcurve.id.as_str())).ok()?;
                let mut copy = pcurve.clone();
                copy.id = id.clone();
                copy.native_ref = None;
                if let PcurveMetadata::General { form } = &pcurve.metadata {
                    let mut metadata = PcurveMetadata::general(
                        form.wrapper_reversed,
                        cadmpeg_ir::units::FiniteVector::new(increasing(directed)),
                        form.fit_tolerance(),
                    );
                    if let PcurveMetadata::General { form: written } = &mut metadata {
                        written.support_surface.clone_from(&form.support_surface);
                        written.uv_offset = form.uv_offset;
                    }
                    copy.metadata = metadata;
                }
                pcurves.push(copy);
                uses[run].push(cadmpeg_ir::topology::PcurveUse {
                    pcurve: id,
                    isoparametric: use_.isoparametric,
                    parameter_range: Some(DirectedParameterRange::new(directed).ok()?),
                });
            }
        }
        let mut order = (0..count).collect::<Vec<_>>();
        if !forward {
            order.reverse();
        }
        let mut ring = Vec::new();
        let mut written = Vec::new();
        for run in order {
            let id = piece_coedge(&coedge.id, run)?;
            written.push(Coedge {
                id: id.clone(),
                owner_loop: coedge.owner_loop.clone(),
                edge: piece_ids[run].clone(),
                radial_next: piece_coedge(&coedge.radial_next, run)?,
                sense: coedge.sense,
                pcurves: std::mem::take(&mut uses[run]),
                use_curve: None,
                native_ref: None,
            });
            ring.push(id);
        }
        coedges.insert(coedge.id.as_str().to_owned(), written);
        rings.insert(coedge.id.as_str(), ring);
    }

    let mut loops = Vec::new();
    for loop_ in &geometry.loops {
        if !loop_
            .coedges()
            .iter()
            .any(|id| rings.contains_key(id.as_str()))
        {
            continue;
        }
        let mut coedge_ids = Vec::new();
        for id in loop_.coedges() {
            match rings.get(id.as_str()) {
                Some(ring) => coedge_ids.extend(ring.iter().cloned()),
                None => coedge_ids.push(id.clone()),
            }
        }
        let vertex_uses = loop_
            .anchored_vertex_uses()
            .iter()
            .cloned()
            .map(|mut vertex_use| {
                if let Some(last) = rings
                    .get(vertex_use.after.as_str())
                    .and_then(|ring| ring.last())
                {
                    vertex_use.after = last.clone();
                }
                vertex_use
            })
            .collect();
        let mut copy = loop_.clone();
        copy.replace_ring(coedge_ids, vertex_uses).ok()?;
        loops.push(copy);
    }

    let taken = |id: &str| {
        geometry.edges.iter().any(|record| record.id.as_str() == id)
            || geometry
                .curves
                .iter()
                .any(|record| record.id.as_str() == id)
            || geometry
                .points
                .iter()
                .any(|record| record.id.as_str() == id)
            || geometry
                .vertices
                .iter()
                .any(|record| record.id.as_str() == id)
            || geometry
                .coedges
                .iter()
                .any(|record| record.id.as_str() == id)
            || geometry
                .pcurves
                .iter()
                .any(|record| record.id.as_str() == id)
    };
    let minted = edges
        .iter()
        .map(|record| record.id.as_str())
        .chain(curves.iter().map(|record| record.id.as_str()))
        .chain(points.iter().map(|record| record.id.as_str()))
        .chain(vertices.iter().map(|record| record.id.as_str()))
        .chain(coedges.values().flatten().map(|record| record.id.as_str()))
        .chain(pcurves.iter().map(|record| record.id.as_str()));
    let mut seen = BTreeSet::new();
    for id in minted {
        if taken(id) || !seen.insert(id) {
            return None;
        }
    }
    let gap = cuts
        .gaps
        .iter()
        .map(|gap| gap.distance)
        .fold(0.0_f64, f64::max);
    let widest = edges
        .iter()
        .filter_map(|piece| piece.tolerance)
        .map(PositiveLength::get)
        .fold(tolerance, f64::max);
    Some(SplitPlan {
        split: EdgeSplit {
            edge: edge.to_owned(),
            pieces: piece_ids.iter().map(|id| id.as_str().to_owned()).collect(),
            gap,
            tolerance: widest,
        },
        edge: edge.to_owned(),
        edges,
        curves,
        points,
        vertices,
        coedges,
        pcurves,
        loops,
    })
}

/// Converts the carriers of the Brep scope `ir`.
pub(super) fn convert_scope(
    ctx: &DecodeContext<'_>,
    compounds: &CompoundIndex<'_>,
    ir: &CadIr,
) -> Result<ConvertedScope, ResourceLimit> {
    let mut unshared = ir.clone();
    let unjoined = unjoin_unpaired_uses(&mut unshared);
    let origins = unshare_carriers(&mut unshared);
    let split = split_gapped_edges(ctx, compounds, &mut unshared)?;
    let ir = &unshared;
    let geometry = &ir.geometry;
    let curves = geometry
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<HashMap<_, _>>();
    let surfaces = geometry
        .surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<HashMap<_, _>>();
    let pcurves = geometry
        .pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<HashMap<_, _>>();
    let edges = geometry
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let coedges = geometry
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<HashMap<_, _>>();
    let loops = geometry
        .loops
        .iter()
        .map(|loop_| (loop_.id.as_str(), loop_))
        .collect::<HashMap<_, _>>();

    let mut curve_users = HashMap::<&str, usize>::new();
    for edge in &geometry.edges {
        if let Some(curve) = edge.curve() {
            *curve_users.entry(curve.as_str()).or_default() += 1;
        }
    }
    let mut edge_curves = BTreeMap::<&str, SolvedCurveGeometry>::new();
    let mut edge_failures = BTreeMap::<&str, String>::new();
    let mut rejoined = Vec::new();
    for edge in &geometry.edges {
        let converted = match edge.curve() {
            None if super::is_singular_edge(edge) => Ok(None),
            None => Err("the edge has no curve".to_owned()),
            Some(id) => match curves.get(id.as_str()) {
                None => Err(format!("its curve {} is missing", id.as_str())),
                Some(curve) => edge_curve(
                    ctx,
                    compounds,
                    edge,
                    curve,
                    super::stated_write_tolerance(edge.tolerance, ir),
                ),
            },
        };
        match converted {
            Ok(None) => {}
            Ok(Some(_)) if edge.curve().is_some_and(|id| curve_users[id.as_str()] > 1) => {
                edge_failures.insert(
                    edge.id.as_str(),
                    "its curve is shared by several edges and needs its own NURBS form".into(),
                );
            }
            Ok(Some((curve, gap))) => {
                if let Some(id) = edge.curve() {
                    edge_curves.insert(id.as_str(), curve);
                }
                if gap > 0.0 {
                    rejoined.push((edge.id.as_str().to_owned(), gap));
                }
            }
            Err(reason) => {
                edge_failures.insert(edge.id.as_str(), reason);
            }
        }
    }

    let mut pcurve_users = HashMap::<&str, usize>::new();
    for coedge in &geometry.coedges {
        for use_ in &coedge.pcurves {
            *pcurve_users.entry(use_.pcurve.as_str()).or_default() += 1;
        }
    }
    let mut dropped = Vec::new();
    let mut plans = BTreeMap::<&str, FacePlan>::new();
    for face in &geometry.faces {
        let plan = (|| -> Result<FacePlan, String> {
            let boundary = face
                .loops
                .boundary()
                .filter(|loops| !loops.is_empty())
                .ok_or(
                    "the face has no boundary loop, and a 3DM Brep face holds an outer trim loop",
                )?;
            let mut face_coedges = Vec::new();
            for id in boundary {
                let loop_ = loops
                    .get(id.as_str())
                    .ok_or_else(|| format!("its loop {} is missing", id.as_str()))?;
                for id in loop_.coedges() {
                    face_coedges.push(
                        *coedges
                            .get(id.as_str())
                            .ok_or_else(|| format!("its coedge {} is missing", id.as_str()))?,
                    );
                }
            }
            for coedge in &face_coedges {
                if let Some(reason) = edge_failures.get(coedge.edge.as_str()) {
                    return Err(format!("edge {}: {reason}", coedge.edge.as_str()));
                }
            }
            let origin = origins
                .get(face.surface.as_str())
                .map_or(face.surface.as_str(), String::as_str);
            let surface = surfaces
                .get(face.surface.as_str())
                .ok_or_else(|| format!("its surface {} is missing", face.surface.as_str()))?;
            let rule = match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
                | SurfaceGeometry::Procedural {
                    cache: Some(SolvedSurfaceGeometry::Plane(_)),
                    ..
                } => PcurveRule::Plane,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
                    if !nurbs.u_periodic() && !nurbs.v_periodic() =>
                {
                    PcurveRule::AsIs
                }
                _ => PcurveRule::Rebuilt,
            };
            let mut face_pcurves = Vec::new();
            for coedge in &face_coedges {
                let edge = edges.get(coedge.edge.as_str());
                let edge_range = edge
                    .and_then(|edge| edge.param_range())
                    .map(cadmpeg_ir::units::FiniteVector::get);
                let singular = edge.is_some_and(|edge| super::is_singular_edge(edge));
                let rule = if singular && rule == PcurveRule::Plane {
                    PcurveRule::AsIs
                } else {
                    rule
                };
                let pcurve = coedge_pcurve(coedge, origin, edge_range, &pcurves, rule)?;
                if singular && pcurve.is_none() {
                    return Err(format!(
                        "coedge {} runs along singular edge {} and has no pcurve",
                        coedge.id.as_str(),
                        coedge.edge.as_str()
                    ));
                }
                face_pcurves.push((coedge.id.as_str().to_owned(), pcurve));
            }
            let splines = face_pcurves
                .iter()
                .filter_map(|(_, pcurve)| pcurve.as_ref()?.spline.clone())
                .collect::<Vec<_>>();
            let written = face_surface(&surface.geometry, &splines)
                .map_err(|reason| format!("surface {}: {reason}", face.surface.as_str()))?;
            let plane = match &written {
                FaceSurface::AsIs => matches!(
                    &surface.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
                ),
                FaceSurface::Same(geometry) => matches!(geometry, SolvedSurfaceGeometry::Plane(_)),
                FaceSurface::Mapped(..) => false,
            };
            for (coedge, pcurve) in &face_pcurves {
                match pcurve {
                    None if !plane => {
                        return Err(format!(
                            "coedge {coedge} has no pcurve on the face's NURBS surface"
                        ))
                    }
                    Some(pcurve)
                        if pcurve.spline.is_some()
                            && pcurve_users.get(pcurve.id.as_str()).copied().unwrap_or(0) > 1 =>
                    {
                        return Err(format!(
                            "pcurve {} is shared by several coedges and needs its own NURBS form",
                            pcurve.id
                        ))
                    }
                    _ => {}
                }
            }
            Ok(FacePlan {
                surface: written,
                pcurves: face_pcurves,
            })
        })();
        match plan {
            Ok(plan) => {
                plans.insert(face.id.as_str(), plan);
            }
            Err(reason) => dropped.push((face.id.as_str().to_owned(), reason)),
        }
    }

    let mut converted = ir.clone();
    let out = &mut converted.geometry;
    let dropped_ids = dropped
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    out.faces
        .retain(|face| !dropped_ids.contains(face.id.as_str()));
    for curve in &mut out.curves {
        if let Some(solved) = edge_curves.remove(curve.id.as_str()) {
            curve.geometry = CurveGeometry::Solved(solved);
        }
    }
    let mut kept_pcurves = BTreeSet::new();
    let mut new_pcurves = BTreeMap::<String, PcurveGeometry>::new();
    let mut coedge_pcurves = BTreeMap::<String, Option<(String, bool)>>::new();
    let mut new_surfaces = BTreeMap::<String, SolvedSurfaceGeometry>::new();
    for face in &out.faces {
        let Some(plan) = plans.remove(face.id.as_str()) else {
            continue;
        };
        let maps = match plan.surface {
            FaceSurface::AsIs => None,
            FaceSurface::Same(geometry) => {
                new_surfaces.insert(face.surface.as_str().to_owned(), geometry);
                None
            }
            FaceSurface::Mapped(geometry, u, v) => {
                new_surfaces.insert(face.surface.as_str().to_owned(), *geometry);
                Some((u, v))
            }
        };
        for (coedge, pcurve) in plan.pcurves {
            let Some(pcurve) = pcurve else {
                coedge_pcurves.insert(coedge, None);
                continue;
            };
            kept_pcurves.insert(pcurve.id.clone());
            if let Some(spline) = pcurve.spline {
                let written = match &maps {
                    Some((u, v)) => exact::composed_pcurve(spline, u, v),
                    None => exact::pcurve_geometry(spline),
                };
                match written {
                    Ok(geometry) => {
                        new_pcurves.insert(pcurve.id.clone(), geometry);
                    }
                    Err(reason) => {
                        dropped.push((
                            face.id.as_str().to_owned(),
                            format!("pcurve {}: {reason}", pcurve.id),
                        ));
                    }
                }
            }
            coedge_pcurves.insert(coedge, Some((pcurve.id, pcurve.against)));
        }
    }
    let late = dropped
        .iter()
        .map(|(id, _)| id.clone())
        .filter(|id| !dropped_ids.contains(id.as_str()))
        .collect::<BTreeSet<_>>();
    if !late.is_empty() {
        out.faces.retain(|face| !late.contains(face.id.as_str()));
    }
    for surface in &mut out.surfaces {
        if let Some(solved) = new_surfaces.remove(surface.id.as_str()) {
            surface.geometry = SurfaceGeometry::Solved(solved);
        }
    }
    for coedge in &mut out.coedges {
        match coedge_pcurves.get(coedge.id.as_str()) {
            Some(Some((id, against))) => {
                coedge.pcurves.retain(|use_| use_.pcurve.as_str() == id);
                coedge.pcurves.truncate(1);
                if let Some(geometry) = new_pcurves.get(id) {
                    if let Some(use_) = coedge.pcurves.first_mut() {
                        use_.parameter_range = if *against {
                            pcurve_knot_domain(geometry).and_then(|[lo, hi]| {
                                cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange::new(
                                    [hi, lo],
                                )
                                .ok()
                            })
                        } else {
                            None
                        };
                    }
                }
            }
            Some(None) => coedge.pcurves.clear(),
            None => {}
        }
    }
    for pcurve in &mut out.pcurves {
        if let Some(geometry) = new_pcurves.remove(pcurve.id.as_str()) {
            let range = pcurve_knot_domain(&geometry);
            let mut metadata = PcurveMetadata::general(
                None,
                range.and_then(cadmpeg_ir::units::FiniteVector::new),
                pcurve.fit_tolerance(),
            );
            if let (PcurveMetadata::General { form }, PcurveMetadata::General { form: source }) =
                (&mut metadata, &pcurve.metadata)
            {
                form.support_surface.clone_from(&source.support_surface);
            }
            pcurve.geometry = geometry;
            pcurve.metadata = metadata;
        }
    }
    let used = out
        .coedges
        .iter()
        .flat_map(|coedge| {
            coedge
                .pcurves
                .iter()
                .map(|use_| use_.pcurve.as_str().to_owned())
        })
        .collect::<BTreeSet<_>>();
    out.pcurves
        .retain(|pcurve| used.contains(pcurve.id.as_str()));
    Ok(ConvertedScope {
        ir: converted,
        findings: ScopeFindings {
            dropped,
            unjoined,
            rejoined,
            split,
        },
    })
}

#[cfg(test)]
mod tests;
