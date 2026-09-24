//! Shared NURBS, B-spline, and analytic-curve math utilities.
//!
//! Family-agnostic geometry math consumed across decode families and the
//! decode/transfer paths: knot expansion and pole counting, degree-5 jet to
//! B-spline conversion, circular
//! interval canonicalization, and exact circular-helix fitting.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsError},
    pcurve::{PcurveGeometry, PcurveNurbs},
    CurveGeometry, FitTolerance, ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::units::{FinitePoint2, FiniteVector, OrthonormalFrame3, UnitVector3};

const EPS_NURBS_COARSE_GEOMETRY: f64 = 1.0e-6;
const EPS_NURBS_GEOMETRY: f64 = 1.0e-9;

const EPS_PERIODIC_SWEEP: f64 = EPS_NURBS_GEOMETRY;
const EPS_HELIX_FRAME: f64 = EPS_NURBS_GEOMETRY;
const EPS_HELIX_RADIUS: f64 = EPS_NURBS_GEOMETRY;
const EPS_HELIX_ORTHO: f64 = EPS_NURBS_GEOMETRY;
const EPS_HELIX_PITCH_ALIGNMENT: f64 = EPS_NURBS_GEOMETRY;
const EPS_RELATIVE_TOLERANCE: f64 = EPS_NURBS_COARSE_GEOMETRY;

fn pcurve_weights_are_positive(nurbs: &PcurveNurbs) -> bool {
    nurbs
        .weights()
        .is_none_or(|weights| weights.iter().all(|weight| weight.get() > 0.0))
}

/// Sink for carrier records whose lanes the IR carrier refuses.
///
/// The IR carrier states which lanes disagree; the codec states which source
/// record stated them. The sink is created once by the top-level decode and
/// threaded through every route, so no `return None`, `?` or `.ok()?` between
/// a refusal and the report can drop it, and N refusals in one document stay N
/// notes. Every reader that records a refusal names its record, so each note
/// carries a source location instead of a bare lane count.
#[derive(Debug, Default)]
pub(crate) struct LaneRefusals {
    notes: Vec<cadmpeg_ir::report::loss::LossNote>,
}

impl LaneRefusals {
    /// An empty sink.
    pub(crate) fn new() -> Self {
        Self { notes: Vec::new() }
    }

    /// Take every refusal recorded so far, in the order the readers stated
    /// them, and leave the sink empty.
    pub(crate) fn take_notes(&mut self) -> Vec<cadmpeg_ir::report::loss::LossNote> {
        std::mem::take(&mut self.notes)
    }

    /// How many refusals the sink holds.
    ///
    /// The router reads this before and after a route, so it can state how
    /// many records a route refused before it transferred no model.
    pub(crate) fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// Retain the identity that prevented a carrier population from merging.
    pub(crate) fn push_annotation_collision(
        &mut self,
        error: &cadmpeg_ir::annotations::AnnotationIdentityCollision,
    ) {
        self.notes
            .push(crate::loss::CatiaLossCode::SourceAnnotationCollision.note(error.to_string()));
    }

    /// Record one refusal against the record that stated it.
    fn push(
        &mut self,
        record: impl std::fmt::Display,
        error: &cadmpeg_ir::geometry::nurbs::NurbsError,
    ) {
        self.notes.push(
            crate::loss::CatiaLossCode::GeometryAnalyticPayloadInvalid.note(format!(
                "A CATIA carrier record states lanes the IR carrier refuses: {record} states {error}"
            )),
        );
    }

    /// Record one refusal a lane solver stated, against the record that stated
    /// the lanes.
    ///
    /// This is the sink for a solver that answers "no representation" without
    /// an IR carrier error, such as a jet whose stored samples do not lower to
    /// a B-spline.
    pub(crate) fn push_solver(&mut self, record: impl std::fmt::Display, detail: &str) {
        self.notes.push(
            crate::loss::CatiaLossCode::GeometryAnalyticPayloadInvalid.note(format!(
                "A CATIA carrier record states lanes the reader cannot lower: {record} {detail}"
            )),
        );
    }

    /// Record a refused source-stated parameter range against the record that
    /// stated it.
    fn push_range(&mut self, record: impl std::fmt::Display, range: [f64; 2]) {
        self.notes.push(
            crate::loss::CatiaLossCode::GeometryParameterRangeInvalid.note(format!(
                "A CATIA record states a parameter range the reader refuses: \
                 {record} states [{}, {}]",
                range[0], range[1]
            )),
        );
    }
}

/// Whether a source-stated parameter range is readable, and a named refusal in
/// the sink when it is not.
///
/// The range is a value the record states, so a non-finite bound, a bound pair
/// that does not increase, or a width that overflows is a refused record, not a
/// record of another kind. `strict` states whether the two bounds must differ.
fn readable_range(range: [f64; 2], strict: bool, refusal: &mut LaneRefusals, record: &str) -> bool {
    let ordered = if strict {
        range[0] < range[1]
    } else {
        range[0] <= range[1]
    };
    if range.into_iter().all(f64::is_finite) && ordered && (range[1] - range[0]).is_finite() {
        return true;
    }
    refusal.push_range(record, range);
    false
}

/// Record a carrier refusal against the record that stated it, and answer
/// `None`.
///
/// A record of the right kind whose lanes the IR carrier refuses is not a
/// record of another kind: the reader answers `None` for the record it is
/// reading, and the refusal travels, named, in the sink the caller owns.
pub(crate) fn note_refusal<T>(
    result: Result<T, cadmpeg_ir::geometry::nurbs::NurbsError>,
    refusal: &mut LaneRefusals,
    record: impl std::fmt::Display,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            refusal.push(record, &error);
            None
        }
    }
}

/// Reverse a supported pcurve over an increasing source-stated range.
///
/// Invalid source ranges and reconstructed NURBS lanes record a refusal.
/// Other failures return `None` for unsupported geometry or a non-finite
/// analytic reconstruction.
pub(crate) fn reverse_pcurve_geometry(
    geometry: &PcurveGeometry,
    range: [f64; 2],
    refusal: &mut LaneRefusals,
    record: &str,
) -> Option<PcurveGeometry> {
    if !readable_range(range, true, refusal, record) {
        return None;
    }
    match geometry {
        PcurveGeometry::Line(line_pcurve) => {
            let origin = line_pcurve.origin().as_raw();
            let direction = line_pcurve.direction().as_raw();
            let origin = FinitePoint2::new(Point2::new(
                range[1].mul_add(direction.u, range[0].mul_add(direction.u, origin.u)),
                range[1].mul_add(direction.v, range[0].mul_add(direction.v, origin.v)),
            ))?;
            Some(PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::new(
                    origin,
                    line_pcurve.direction().reversed(),
                ),
            ))
        }
        PcurveGeometry::Nurbs { nurbs } => {
            if !pcurve_weights_are_positive(nurbs) {
                return None;
            }
            let reversed_knots = reverse_knots(nurbs.knots(), range);
            let mut poles = nurbs.pole_rows().clone();
            poles.reverse();
            match PcurveNurbs::new(nurbs.degree(), reversed_knots, poles, nurbs.periodic()) {
                Ok(nurbs) => Some(PcurveGeometry::Nurbs { nurbs }),
                Err(error) => note_refusal(Err(error), refusal, record),
            }
        }
        _ => None,
    }
}

/// Reverse a supported model-space curve over an increasing native range.
///
/// Invalid source ranges and reconstructed NURBS lanes record a refusal,
/// as in [`reverse_pcurve_geometry`]. Unsupported geometry and non-finite
/// analytic reconstructions return `None`.
pub(crate) fn reverse_curve_geometry(
    geometry: &CurveGeometry,
    range: [f64; 2],
    refusal: &mut LaneRefusals,
    record: &str,
) -> Option<(CurveGeometry, [f64; 2])> {
    if !readable_range(range, false, refusal, record) {
        return None;
    }
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let direction = line_curve.direction();
            let length = range[1] - range[0];
            let origin = FinitePoint3::new(origin.translated(*direction.as_raw(), range[1]))?;
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::new(origin, direction.reversed()),
                )),
                [0.0, length],
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let axis = *circle_curve.frame().axis().as_raw();
            let reference = *circle_curve.frame().reference().as_raw();
            let sweep = range[1] - range[0];
            let tangent = axis.cross(reference);
            let end = range[1];
            let reference = reference.scale(end.cos()) + tangent.scale(end.sin());
            let frame = OrthonormalFrame3::from_units(
                circle_curve.frame().axis().reversed(),
                UnitVector3::new(reference)?,
            )?;
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::new(
                        circle_curve.center(),
                        frame,
                        circle_curve.radius(),
                    ),
                )),
                [0.0, sweep],
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            match reverse_nurbs_curve(nurbs, range) {
                Ok(curve) => Some((
                    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                    range,
                )),
                Err(error) => note_refusal(Err(error), refusal, record),
            }
        }
        _ => None,
    }
}

/// Reflect knots without summing the interval endpoints. Subtracting from
/// the nearer endpoint preserves small spans at large parameter offsets.
fn reverse_knots(knots: &[f64], [lower, upper]: [f64; 2]) -> Vec<f64> {
    knots
        .iter()
        .rev()
        .map(|&knot| {
            let reflected = if (knot - lower).abs() <= (upper - knot).abs() {
                upper - (knot - lower)
            } else {
                lower + (upper - knot)
            };
            if reflected == 0.0 {
                0.0
            } else {
                reflected
            }
        })
        .collect()
}

/// Reverse a NURBS carrier in the stated parameter chart.
pub(crate) fn reverse_nurbs_curve(
    curve: &NurbsCurve,
    range: [f64; 2],
) -> Result<NurbsCurve, NurbsError> {
    if !range.into_iter().all(f64::is_finite) || range[0] > range[1] {
        return Err(NurbsError::Structure(
            "reversal range must be finite and ordered".into(),
        ));
    }
    let mut poles = curve.pole_rows().clone();
    poles.reverse();
    NurbsCurve::new(
        curve.degree(),
        reverse_knots(curve.knots(), range),
        poles,
        curve.periodic(),
    )
}

/// State one trim endpoint inside the carrier domain, or refuse it.
///
/// The record states the interval and the IR carrier states the domain, so the
/// two agree only inside the parameter rounding the knot lane carries.
/// `tolerance` names that band. An endpoint beyond the band states a different
/// interval rather than a rounded one, so it is refused; only the excess the
/// band admits is mapped onto the domain end. A domain the carrier states in
/// the wrong order admits no endpoint at all.
fn domain_endpoint(parameter: f64, [lower, upper]: [f64; 2], tolerance: f64) -> Option<f64> {
    if !(lower - tolerance..=upper + tolerance).contains(&parameter) {
        return None;
    }
    if parameter < lower {
        return Some(lower);
    }
    if parameter > upper {
        return Some(upper);
    }
    Some(parameter)
}

/// Normalize the parameter interval for a model-space carrier.
///
/// The interval is a value the record states, so a non-finite bound, a bound
/// pair that does not increase, and a circular sweep this reader cannot
/// normalize are each a refused record named in the sink, not a record of
/// another kind. The remaining `None` exits re-read a domain the IR carrier
/// already refined.
pub(crate) fn canonical_model_curve_range(
    geometry: &CurveGeometry,
    range: [f64; 2],
    refusal: &mut LaneRefusals,
    record: &str,
) -> Option<[f64; 2]> {
    if !readable_range(range, false, refusal, record) {
        return None;
    }
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)) => {
            let normalized = canonical_periodic_range(range);
            if normalized.is_none() {
                refusal.push_range(record, range);
            }
            normalized
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let [lower, upper] = cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)?.endpoints();
            let tolerance = EPS_NURBS_GEOMETRY.max((upper - lower).abs() * EPS_NURBS_GEOMETRY);
            if nurbs.periodic() {
                (range[1] - range[0] <= upper - lower + tolerance).then_some(range)
            } else {
                // Correct only endpoints outside the domain; retain interior trims.
                let [start, end] = range;
                Some([
                    domain_endpoint(start, [lower, upper], tolerance)?,
                    domain_endpoint(end, [lower, upper], tolerance)?,
                ])
            }
        }
        _ => Some(range),
    }
}

/// Reverse a cone-helix construction over its complete angular domain.
///
/// The construction uses the angle itself as the curve parameter. Reversing
/// the interval therefore changes the radial frame, axial rise, and handedness
/// while preserving the same increasing parameter range. Other procedural
/// curve families require family-specific support-side mappings and are not
/// admitted here.
pub(crate) fn reverse_helix_definition(
    definition: &ProceduralCurveDefinition,
    range: [f64; 2],
    refusal: &mut LaneRefusals,
    record: &str,
) -> Option<(ProceduralCurveDefinition, [f64; 2])> {
    let ProceduralCurveDefinition::Helix(helix_payload) = definition else {
        return None;
    };
    if !readable_range(range, true, refusal, record) {
        return None;
    }
    let angle_range = helix_payload.angle_range();
    let center = helix_payload.center().as_raw();
    let major = helix_payload.major();
    let minor = helix_payload.minor();
    let pitch = helix_payload.pitch();
    let apex_factor = helix_payload.apex_factor();
    let axis = helix_payload.axis();

    if range != angle_range.get() {
        return None;
    }
    let revolutions = (range[1] - range[0]) / std::f64::consts::TAU;
    let radial_scale_at_end = 1.0 + apex_factor.get() * revolutions;
    if !revolutions.is_finite() || !radial_scale_at_end.is_finite() || radial_scale_at_end == 0.0 {
        return None;
    }
    let angle_sum = range[0] + range[1];
    if !angle_sum.is_finite() {
        return None;
    }
    let major_at_end = major.scale(angle_sum.cos()) + minor.scale(angle_sum.sin());
    let minor_at_end = major.scale(angle_sum.sin()) - minor.scale(angle_sum.cos());
    let major = major_at_end.scale(radial_scale_at_end);
    let minor = minor_at_end.scale(radial_scale_at_end);
    let center = center.translated(pitch.get(), revolutions);
    let pitch = pitch.scale(-1.0);
    let apex_factor = -apex_factor.get() / radial_scale_at_end;
    let axis = axis.scale(-1.0);
    if ![center.x, center.y, center.z, apex_factor]
        .into_iter()
        .chain(
            [major, minor, pitch, axis]
                .into_iter()
                .flat_map(|vector| [vector.x, vector.y, vector.z]),
        )
        .all(f64::is_finite)
    {
        return None;
    }
    Some((
        ProceduralCurveDefinition::Helix(
            cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                angle_range.get(),
                cadmpeg_ir::geometry::HelixFrame {
                    center,
                    major,
                    minor,
                    pitch,
                    axis,
                },
                apex_factor,
                None,
            )
            .ok()?,
        ),
        range,
    ))
}

/// Normalize an increasing circular interval to the canonical one-turn domain.
pub(crate) fn canonical_periodic_range(range: [f64; 2]) -> Option<[f64; 2]> {
    let sweep = range[1] - range[0];
    if !sweep.is_finite() || sweep <= 0.0 || sweep > std::f64::consts::TAU + EPS_PERIODIC_SWEEP {
        return None;
    }
    let mut start = range[0].rem_euclid(std::f64::consts::TAU);
    if std::f64::consts::TAU - start <= EPS_PERIODIC_SWEEP {
        start = 0.0;
    }
    Some([start, start + sweep])
}

/// Angle-parameterized degree-1 cache for an exact circular helix.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CircularHelixCache {
    /// Piecewise-linear curve cache on the construction's angle interval.
    pub(crate) curve: NurbsCurve,
    /// Maximum radial sagitta deviation in model length units.
    pub(crate) fit_tolerance: FitTolerance,
}

/// Fit a circular helix with a bounded angle-parameterized polyline cache.
///
/// Every `return None` here states that the construction is not an exact
/// circular helix this cache covers, not that the record is refused: the curve
/// still transfers, without a solved cache. Two answers are refusals and do
/// reach the sink: the source-stated angle interval, which must be finite and
/// increasing, and the lane refusal from `NurbsCurve::from_lanes`.
pub(crate) fn circular_helix_cache(
    construction: &ProceduralCurveDefinition,
    requested_tolerance: PositiveReal,
    refusal: &mut LaneRefusals,
    record: &str,
) -> Option<CircularHelixCache> {
    let requested_tolerance = requested_tolerance.get();
    let ProceduralCurveDefinition::Helix(helix_payload) = construction else {
        return None;
    };
    let angle_range = helix_payload.angle_range();
    if !readable_range(angle_range.get(), true, refusal, record) {
        return None;
    }
    let major = helix_payload.major();
    let minor = helix_payload.minor();
    let pitch = helix_payload.pitch();
    let apex_factor = helix_payload.apex_factor();
    let axis = helix_payload.axis();

    let axis_norm = axis.x.hypot(axis.y).hypot(axis.z);
    let radius = major.x.hypot(major.y).hypot(major.z);
    let minor_radius = minor.x.hypot(minor.y).hypot(minor.z);
    let pitch_norm = pitch.x.hypot(pitch.y).hypot(pitch.z);
    let normalized_dot = |left: &Vector3, right: &Vector3| {
        (left.x / left.x.hypot(left.y).hypot(left.z))
            * (right.x / right.x.hypot(right.y).hypot(right.z))
            + (left.y / left.x.hypot(left.y).hypot(left.z))
                * (right.y / right.x.hypot(right.y).hypot(right.z))
            + (left.z / left.x.hypot(left.y).hypot(left.z))
                * (right.z / right.x.hypot(right.y).hypot(right.z))
    };
    if !radius.is_finite()
        || radius <= 0.0
        || !minor_radius.is_finite()
        || minor_radius <= 0.0
        || !axis_norm.is_finite()
        || (axis_norm - 1.0).abs() > EPS_HELIX_FRAME
        || !pitch_norm.is_finite()
        || (radius - minor_radius).abs() > EPS_HELIX_RADIUS * radius.max(minor_radius)
        || apex_factor.get() != 0.0
    {
        return None;
    }
    let normalized_dot_major_minor = normalized_dot(major, minor);
    let normalized_dot_major_axis = normalized_dot(major, axis);
    let normalized_dot_minor_axis = normalized_dot(minor, axis);
    let normalized_dot_pitch_axis = if pitch_norm == 0.0 {
        1.0
    } else {
        normalized_dot(pitch, axis)
    };
    if !normalized_dot_major_minor.is_finite()
        || normalized_dot_major_minor.abs() > EPS_HELIX_ORTHO
        || !normalized_dot_major_axis.is_finite()
        || normalized_dot_major_axis.abs() > EPS_HELIX_ORTHO
        || !normalized_dot_minor_axis.is_finite()
        || normalized_dot_minor_axis.abs() > EPS_HELIX_ORTHO
        || !normalized_dot_pitch_axis.is_finite()
        || normalized_dot_pitch_axis.abs() < 1.0 - EPS_HELIX_PITCH_ALIGNMENT
    {
        return None;
    }
    let sweep = angle_range[1] - angle_range[0];
    if !sweep.is_finite() || sweep <= 0.0 {
        return None;
    }
    let relative_tolerance = requested_tolerance / radius;
    // The step whose chord sagitta is the requested tolerance is
    // `2 * acos(1 - relative_tolerance)`. The requested tolerance is positive
    // and the refusals above state `radius > 0`, so the relative tolerance is
    // positive and `1 - relative_tolerance` never passes `acos`'s upper bound.
    // It passes the lower bound when the tolerance reaches the diameter, and
    // that is a stated step rather than a value out of domain: a tolerance at
    // or past the diameter bounds every chord of the circle, so the step it
    // states is the whole turn. Each of the three states has its own arm here.
    let max_step = if relative_tolerance < EPS_RELATIVE_TOLERANCE {
        2.0 * (2.0 * relative_tolerance).sqrt()
    } else if relative_tolerance <= 2.0 {
        2.0 * (1.0 - relative_tolerance).acos()
    } else {
        2.0 * std::f64::consts::PI
    };
    if !max_step.is_finite() || max_step <= 0.0 {
        return None;
    }
    // Both refusals above bound the quotient: `sweep` and `max_step` are each
    // finite and positive, so the quotient is positive and ceils to at least
    // one segment. No floor stands here.
    let segment_count = (sweep / max_step).ceil();
    if !segment_count.is_finite() || segment_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let segment_count = segment_count as usize;
    let step = sweep / segment_count as f64;
    let samples = (0..=segment_count)
        .map(|index| {
            let parameter = if index == segment_count {
                angle_range[1]
            } else {
                angle_range[0] + index as f64 * step
            };
            if !parameter.is_finite() {
                return None;
            }
            Some((parameter, circular_helix_point(construction, parameter)?))
        })
        .collect::<Option<Vec<_>>>()?;
    if !samples
        .windows(2)
        .all(|pair| pair[0].0.is_finite() && pair[0].0 < pair[1].0)
    {
        return None;
    }
    let sine = (step * 0.25).sin();
    let fit_tolerance = (radius * sine) * (2.0 * sine);
    let mut knots = Vec::with_capacity(samples.len() + 2);
    knots.push(angle_range[0]);
    knots.extend(samples.iter().map(|(parameter, _)| *parameter));
    knots.push(angle_range[1]);
    let curve = match NurbsCurve::from_lanes(
        1,
        knots,
        samples.into_iter().map(|(_, point)| point).collect(),
        None,
        false,
    ) {
        Ok(curve) => curve,
        Err(error) => return note_refusal(Err(error), refusal, record),
    };
    Some(CircularHelixCache {
        curve,
        fit_tolerance: FitTolerance::try_new(fit_tolerance).ok()?,
    })
}

fn circular_helix_point(construction: &ProceduralCurveDefinition, angle: f64) -> Option<Point3> {
    let ProceduralCurveDefinition::Helix(helix_payload) = construction else {
        return None;
    };
    let angle_range = helix_payload.angle_range();
    let center = helix_payload.center().as_raw();
    let major = helix_payload.major();
    let minor = helix_payload.minor();
    let pitch = helix_payload.pitch();

    if !angle.is_finite() || angle_range[0] >= angle_range[1] {
        return None;
    }
    let revolution_fraction = (angle - angle_range[0]) / std::f64::consts::TAU;
    if !revolution_fraction.is_finite() {
        return None;
    }
    let point = Point3::new(
        center.x + major.x * angle.cos() + minor.x * angle.sin() + pitch.x * revolution_fraction,
        center.y + major.y * angle.cos() + minor.y * angle.sin() + pitch.y * revolution_fraction,
        center.z + major.z * angle.cos() + minor.z * angle.sin() + pitch.z * revolution_fraction,
    );
    point.is_finite().then_some(point)
}

/// Convert degree-5 position/first/second-derivative knot jets into an exact
/// piecewise Bézier B-spline control net, in any point dimension.
pub(crate) fn quintic_jet_bspline<const N: usize>(
    degree: u32,
    knots: &[f64],
    points: &[[f64; N]],
    first: &[[f64; N]],
    second: &[[f64; N]],
) -> Option<(Vec<f64>, Vec<FiniteVector<N>>)> {
    if degree != 5
        || knots.len() < 2
        || points.len() != knots.len()
        || first.len() != knots.len()
        || second.len() != knots.len()
        || !knots.iter().copied().all(f64::is_finite)
        || !points.iter().flatten().copied().all(f64::is_finite)
        || !first.iter().flatten().copied().all(f64::is_finite)
        || !second.iter().flatten().copied().all(f64::is_finite)
    {
        return None;
    }
    let mut controls = Vec::with_capacity(6 * (knots.len() - 1));
    let mut full_knots = vec![knots[0]; 6];
    for index in 0..knots.len() - 1 {
        let h = knots[index + 1] - knots[index];
        if !h.is_finite() || h <= 0.0 {
            return None;
        }
        let p0 = points[index];
        let p1 = points[index + 1];
        let d0 = first[index];
        let d1 = first[index + 1];
        let dd0 = second[index];
        let dd1 = second[index + 1];
        // Scale derivatives before squaring the span. In particular, h*h
        // can overflow while h*h*dd is finite, including when dd is zero.
        let tangent = |derivative: f64| {
            let product = h * derivative;
            if product.is_finite() {
                product / 5.0
            } else {
                (h / 5.0) * derivative
            }
        };
        let curvature = |derivative: f64| {
            let product = h * derivative;
            if product.is_finite() {
                (h / 20.0) * product
            } else {
                ((h / 20.0) * derivative) * h
            }
        };
        controls.extend([
            p0,
            std::array::from_fn(|axis| p0[axis] + tangent(d0[axis])),
            std::array::from_fn(|axis| {
                tangent(d0[axis]).mul_add(2.0, p0[axis]) + curvature(dd0[axis])
            }),
            std::array::from_fn(|axis| {
                tangent(d1[axis]).mul_add(-2.0, p1[axis]) + curvature(dd1[axis])
            }),
            std::array::from_fn(|axis| p1[axis] - tangent(d1[axis])),
            p1,
        ]);
        full_knots.extend([knots[index + 1]; 6]);
    }
    if !full_knots.iter().copied().all(f64::is_finite) {
        return None;
    }
    let controls = controls
        .into_iter()
        .map(FiniteVector::new)
        .collect::<Option<Vec<_>>>()?;
    Some((full_knots, controls))
}

pub(crate) fn expand_knots(distinct: &[f64], multiplicities: &[u32]) -> Option<Vec<f64>> {
    let capacity = multiplicities
        .iter()
        .try_fold(0usize, |sum, value| sum.checked_add(*value as usize))?;
    let mut knots = Vec::with_capacity(capacity);
    for (&knot, &multiplicity) in distinct.iter().zip(multiplicities) {
        knots.extend(std::iter::repeat_n(knot, multiplicity as usize));
    }
    Some(knots)
}

pub(crate) fn pole_count(multiplicities: &[u32], degree: u32) -> Option<u32> {
    multiplicities
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(degree.checked_add(1)?)
}

#[cfg(test)]
mod tests {
    mod numerical_limits;
    use cadmpeg_ir::eval::{curve_point, pcurve_uv};
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsCurve, NurbsSurface},
        pcurve::PcurveGeometry,
        CurveGeometry, ProceduralCurveDefinition, SolvedCurveGeometry,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::scalar::PositiveReal;

    use super::{
        canonical_model_curve_range, circular_helix_cache, quintic_jet_bspline,
        reverse_curve_geometry, reverse_helix_definition, reverse_pcurve_geometry, LaneRefusals,
    };
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;

    const DOMAIN_ROUNDING: f64 = 1.0e-12;

    #[test]
    // These checked constructors must accept the explicit test fixtures.
    #[allow(clippy::unwrap_used)]
    fn canonical_nurbs_range_clamps_rounding_at_the_domain_boundary() {
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        ));

        let mut refusal = LaneRefusals::new();
        assert_eq!(
            canonical_model_curve_range(
                &geometry,
                [-DOMAIN_ROUNDING, 1.0 + DOMAIN_ROUNDING],
                &mut refusal,
                "test curve"
            ),
            Some([0.0, 1.0])
        );
        for (range, expected) in [
            ([-DOMAIN_ROUNDING, 0.5], [0.0, 0.5]),
            ([0.5, 1.0 + DOMAIN_ROUNDING], [0.5, 1.0]),
            ([0.25, 0.75], [0.25, 0.75]),
            (
                [-DOMAIN_ROUNDING, 1.0 - DOMAIN_ROUNDING],
                [0.0, 1.0 - DOMAIN_ROUNDING],
            ),
            (
                [DOMAIN_ROUNDING, 1.0 + DOMAIN_ROUNDING],
                [DOMAIN_ROUNDING, 1.0],
            ),
        ] {
            assert_eq!(
                canonical_model_curve_range(&geometry, range, &mut refusal, "test curve"),
                Some(expected)
            );
        }
        for range in [[-1.0e-4, 0.5], [0.5, 1.0 + 1.0e-4]] {
            assert_eq!(
                canonical_model_curve_range(&geometry, range, &mut refusal, "test curve"),
                None
            );
        }
        assert_eq!(
            canonical_model_curve_range(&geometry, [-1.0e-4, 1.0], &mut refusal, "test curve"),
            None
        );
        // Domain corrections do not add a refused source range.
        assert!(refusal.take_notes().is_empty());

        // A source-stated interval that does not increase is a refused record.
        assert_eq!(
            canonical_model_curve_range(&geometry, [1.0, 0.0], &mut refusal, "test curve"),
            None
        );
        let notes = refusal.take_notes();
        assert_eq!(notes.len(), 1);
        assert!(
            notes[0].message.contains("test curve states [1, 0]"),
            "{:?}",
            notes[0]
        );
    }

    #[test]
    fn reversed_surface_pcurve_preserves_domain_and_swaps_endpoints() {
        let geometry = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(2.0, -1.0),
                Point2::new(3.0, 4.0),
            )
            .expect("valid LinePcurve fixture"),
        );
        let range = [5.0, 9.0];
        let reversed = reverse_pcurve_geometry(
            &geometry,
            range,
            &mut crate::nurbs::LaneRefusals::new(),
            "test record",
        )
        .expect("reversible line");
        for (parameter, source_parameter) in [(5.0, 9.0), (9.0, 5.0)] {
            let actual = pcurve_uv(&reversed, parameter).expect("reversed evaluation");
            let expected = pcurve_uv(&geometry, source_parameter).expect("source evaluation");
            assert!((actual.u - expected.u).abs() < 1.0e-12);
            assert!((actual.v - expected.v).abs() < 1.0e-12);
        }
    }

    #[test]
    fn reversed_model_carriers_preserve_endpoint_geometry() {
        let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(2.0, -1.0, 4.0),
                Vector3::new(3.0, 4.0, -2.0)
                    .unit()
                    .expect("nonzero fixture direction"),
            )
            .expect("valid LineCurve fixture"),
        ));
        let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(2.0, -1.0, 4.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                3.0,
            )
            .expect("valid CircleCurve fixture"),
        ));
        for (geometry, range) in [(line, [5.0, 9.0]), (circle, [0.25, 2.0])] {
            let (reversed, reversed_range) = reverse_curve_geometry(
                &geometry,
                range,
                &mut crate::nurbs::LaneRefusals::new(),
                "test record",
            )
            .expect("reversible model curve");
            for (parameter, source_parameter) in
                [(reversed_range[0], range[1]), (reversed_range[1], range[0])]
            {
                let actual = curve_point(&reversed, parameter).expect("reversed endpoint");
                let expected = curve_point(&geometry, source_parameter).expect("source endpoint");
                assert!(actual.distance(expected.get()) < 1.0e-12);
            }
        }
    }

    #[test]
    // These checked constructors must accept the explicit test fixtures.
    #[allow(clippy::unwrap_used)]
    fn reversed_nurbs_preserves_active_subrange() {
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        ));
        let range = [0.2, 0.8];
        let (reversed, reversed_range) = reverse_curve_geometry(
            &geometry,
            range,
            &mut crate::nurbs::LaneRefusals::new(),
            "test record",
        )
        .expect("reversible NURBS");
        for parameter in [range[0], 0.5, range[1]] {
            let actual = curve_point(&reversed, parameter).expect("reversed NURBS point");
            let expected = curve_point(&geometry, range[0] + range[1] - parameter)
                .expect("source NURBS point");
            assert!(actual.distance(expected.get()) < 1.0e-12);
        }
        assert_eq!(reversed_range, range);
    }

    #[test]
    fn reversed_helix_preserves_conical_path() {
        let range = [0.25, 2.0];
        let definition = ProceduralCurveDefinition::Helix(
            cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                range,
                cadmpeg_ir::geometry::HelixFrame {
                    center: Point3::new(1.0, -2.0, 3.0),
                    major: Vector3::new(2.0, 0.0, 0.0),
                    minor: Vector3::new(0.0, 2.0, 0.0),
                    pitch: Vector3::new(0.0, 0.0, 3.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                0.4,
                None,
            )
            .expect("valid HelixCurveConstruction fixture"),
        );
        let mut refusal = LaneRefusals::new();
        let (reversed, reversed_range) =
            reverse_helix_definition(&definition, range, &mut refusal, "test helix")
                .expect("reversible helix");
        assert!(refusal.take_notes().is_empty());
        let evaluate = |definition: &ProceduralCurveDefinition, angle: f64| {
            let ProceduralCurveDefinition::Helix(helix_payload) = definition else {
                panic!("helix definition")
            };
            let angle_range = helix_payload.angle_range();
            let center = helix_payload.center().as_raw();
            let major = helix_payload.major();
            let minor = helix_payload.minor();
            let pitch = helix_payload.pitch();
            let apex_factor = helix_payload.apex_factor();

            let fraction = (angle - angle_range[0]) / std::f64::consts::TAU;
            let scale = 1.0 + apex_factor.get() * fraction;
            center
                .translated(major.get(), scale * angle.cos())
                .translated(minor.get(), scale * angle.sin())
                .translated(pitch.get(), fraction)
        };
        for angle in [range[0], 0.75, range[1]] {
            let actual = evaluate(&reversed, angle);
            let expected = evaluate(&definition, range[0] + range[1] - angle);
            assert!(actual.distance(expected) < 1.0e-12);
        }
        assert_eq!(reversed_range, range);
    }

    #[test]
    // These checked constructors must accept the explicit test fixtures.
    #[allow(clippy::unwrap_used)]
    fn surface_isocurve_preserves_tiny_weights_and_knot_domain() {
        let tiny = 1e-200;
        let surface = NurbsSurface::from_lanes(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                1,
                vec![0.0, 0.0, tiny, tiny],
                false,
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                    vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 1.0, 0.0)],
                ],
                Some(vec![tiny; 4])
                    .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
            ),
            false,
        )
        .unwrap();
        let curve = cadmpeg_ir::eval::nurbs_surface_isocurve(
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
            tiny * 0.5,
        )
        .expect("tiny rational surface isocurve");
        assert_eq!(
            curve.control_points(),
            [Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)]
        );
        assert_eq!(curve.pole_rows().weights(), Some(vec![tiny, tiny]));
    }

    #[test]
    // These checked constructors must accept the explicit test fixtures.
    #[allow(clippy::unwrap_used)]
    fn surface_isocurve_rejects_nonfinite_output() {
        let surface = |control_points: Vec<Point3>, weights: Option<Vec<f64>>| {
            NurbsSurface::from_lanes(
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                    control_points.chunks(2).map(<[_]>::to_vec).collect(),
                    weights.map(|values| values.chunks(2).map(<[_]>::to_vec).collect()),
                ),
                false,
            )
            .unwrap()
        };
        assert!(cadmpeg_ir::eval::nurbs_surface_isocurve(
            &surface(
                vec![
                    Point3::new(f64::MAX, 0.0, 0.0),
                    Point3::new(f64::MAX, 0.0, 0.0),
                    Point3::new(-f64::MAX, 0.0, 0.0),
                    Point3::new(-f64::MAX, 0.0, 0.0),
                ],
                Some(vec![1.0, 1.0, -0.5, -0.5]),
            ),
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
            0.5
        )
        .is_none());
    }

    #[test]
    fn circular_helix_cache_preserves_exact_interval_endpoints() {
        let range = [0.125, 1.570_797_917_999_999_6];
        let definition = ProceduralCurveDefinition::Helix(
            cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                range,
                cadmpeg_ir::geometry::HelixFrame {
                    center: Point3::new(0.0, 0.0, 0.0),
                    major: Vector3::new(1.0, 0.0, 0.0),
                    minor: Vector3::new(0.0, 1.0, 0.0),
                    pitch: Vector3::new(0.0, 0.0, 1.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                0.0,
                None,
            )
            .expect("valid HelixCurveConstruction fixture"),
        );

        let cache = circular_helix_cache(
            &definition,
            PositiveReal::new(1.0e-4).expect("positive fixture tolerance"),
            &mut crate::nurbs::LaneRefusals::new(),
            "test record",
        )
        .expect("valid helix");
        assert_eq!(cache.curve.knots()[1], range[0]);
        assert_eq!(cache.curve.knots()[cache.curve.knots().len() - 2], range[1]);
        assert!(cache.fit_tolerance.get() > 0.0);
    }

    /// A tolerance at or past the diameter bounds every chord, so the step it
    /// states is the whole turn.
    ///
    /// The three arms are value-identical to the clamp they replaced on every
    /// input this route admits, so no test separates the shapes. What proves
    /// the clamp is gone is the census: no clamp remains in this file.
    /// This test states the value the whole-turn arm answers.
    #[test]
    fn a_relative_tolerance_at_or_past_the_diameter_states_the_whole_turn() {
        // The sweep is longer than a half turn and shorter than a whole one,
        // so a step of the whole turn states one segment and any shorter step
        // states more than one.
        let range = [0.0, 4.0];
        let definition = ProceduralCurveDefinition::Helix(
            cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                range,
                cadmpeg_ir::geometry::HelixFrame {
                    center: Point3::new(0.0, 0.0, 0.0),
                    major: Vector3::new(1.0, 0.0, 0.0),
                    minor: Vector3::new(0.0, 1.0, 0.0),
                    pitch: Vector3::new(0.0, 0.0, 1.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                0.0,
                None,
            )
            .expect("valid HelixCurveConstruction fixture"),
        );
        // The major axis is a unit vector, so the radius is one and the
        // requested tolerance is the relative tolerance.
        let cache = |tolerance| {
            circular_helix_cache(
                &definition,
                PositiveReal::new(tolerance).expect("positive fixture tolerance"),
                &mut crate::nurbs::LaneRefusals::new(),
                "test record",
            )
            .expect("a stated step")
        };
        let fine = cache(1.0e-4);
        let diameter = cache(2.0);
        let past = cache(2.5);
        assert_eq!(past.curve.knots(), diameter.curve.knots());
        assert_eq!(
            past.curve.control_points(),
            diameter.curve.control_points(),
            "the diameter and every tolerance past it state one step"
        );
        assert!(fine.curve.control_points().len() > past.curve.control_points().len());
    }

    #[test]
    fn circular_helix_frame_validation_is_scale_independent() {
        const SMALL_ADMITTED_HELIX_RADIUS: f64 = 1.0e-10;
        let radius = SMALL_ADMITTED_HELIX_RADIUS;
        let definition = |minor| {
            ProceduralCurveDefinition::Helix(
                cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                    [0.0, 1.0],
                    cadmpeg_ir::geometry::HelixFrame {
                        center: Point3::new(0.0, 0.0, 0.0),
                        major: Vector3::new(radius, 0.0, 0.0),
                        minor,
                        pitch: Vector3::new(0.0, 0.0, 1.0),
                        axis: Vector3::new(0.0, 0.0, 1.0),
                    },
                    0.0,
                    None,
                )
                .expect("valid HelixCurveConstruction fixture"),
            )
        };

        assert!(circular_helix_cache(
            &definition(Vector3::new(0.0, radius, 0.0)),
            PositiveReal::new(1.0e-4).expect("positive fixture tolerance"),
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_some());
        assert!(circular_helix_cache(
            &definition(Vector3::new(0.0, 2.0 * radius, 0.0)),
            PositiveReal::new(1.0e-4).expect("positive fixture tolerance"),
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());
        assert!(circular_helix_cache(
            &definition(Vector3::new(radius, 0.0, 0.0)),
            PositiveReal::new(1.0e-4).expect("positive fixture tolerance"),
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());
    }

    #[test]
    fn circular_helix_cache_rejects_invalid_frame_and_output() {
        let definition = ProceduralCurveDefinition::Helix(
            cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                [0.0, 1.0],
                cadmpeg_ir::geometry::HelixFrame {
                    center: Point3::new(0.0, 0.0, 0.0),
                    major: Vector3::new(1.0, 0.0, 0.0),
                    minor: Vector3::new(0.0, 1.0, 0.0),
                    pitch: Vector3::new(0.0, 0.0, 1.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                0.0,
                None,
            )
            .expect("valid HelixCurveConstruction fixture"),
        );
        let mut non_axial_pitch = definition.clone();
        if let ProceduralCurveDefinition::Helix(helix_payload) = &mut non_axial_pitch {
            let angle_range = *helix_payload.angle_range();
            let center = *helix_payload.center().as_raw();
            let major = *helix_payload.major();
            let minor = *helix_payload.minor();
            let apex_factor = helix_payload.apex_factor();
            let axis = *helix_payload.axis();
            *helix_payload = cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                angle_range.get(),
                cadmpeg_ir::geometry::HelixFrame {
                    center,
                    major: major.get(),
                    minor: minor.get(),
                    pitch: Vector3::new(1.0, 0.0, 0.0),
                    axis: axis.get(),
                },
                apex_factor.get(),
                None,
            )
            .expect("valid HelixCurveConstruction fixture");
        }
        assert!(circular_helix_cache(
            &non_axial_pitch,
            PositiveReal::new(1.0e-4).expect("positive fixture tolerance"),
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());

        let overflowing_points = ProceduralCurveDefinition::Helix(
            cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                [0.0, 1.0],
                cadmpeg_ir::geometry::HelixFrame {
                    center: Point3::new(f64::MAX, 0.0, 0.0),
                    major: Vector3::new(f64::MAX, 0.0, 0.0),
                    minor: Vector3::new(0.0, f64::MAX, 0.0),
                    pitch: Vector3::new(0.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                0.0,
                None,
            )
            .expect("valid HelixCurveConstruction fixture"),
        );
        assert!(circular_helix_cache(
            &overflowing_points,
            PositiveReal::new(f64::MAX).expect("positive fixture tolerance"),
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());
    }

    #[test]
    fn quintic_jet_rejects_nonfinite_control_net() {
        assert!(quintic_jet_bspline(
            5,
            &[0.0, 10.0],
            &[[0.0, 0.0], [1.0, 0.0]],
            &[[f64::MAX, 0.0], [f64::MAX, 0.0]],
            &[[0.0, 0.0], [0.0, 0.0]],
        )
        .is_none());
        assert!(quintic_jet_bspline(
            5,
            &[0.0, 1.0],
            &[[f64::NAN, 0.0], [1.0, 0.0]],
            &[[1.0, 0.0], [1.0, 0.0]],
            &[[0.0, 0.0], [0.0, 0.0]],
        )
        .is_none());
    }

    #[test]
    // These checked constructors must accept the explicit test fixtures.
    #[allow(clippy::unwrap_used)]
    fn reversing_geometry_rejects_nonfinite_reconstruction() {
        let pcurve_line = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
            )
            .unwrap(),
        );
        assert!(reverse_pcurve_geometry(
            &pcurve_line,
            [f64::MAX / 2.0, f64::MAX],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());

        let model_line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(f64::MAX, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ));
        assert!(reverse_curve_geometry(
            &model_line,
            [0.0, f64::MAX],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());

        let pcurve_nurbs = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                1,
                vec![-f64::MAX, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        };
        assert!(reverse_pcurve_geometry(
            &pcurve_nurbs,
            [0.0, f64::MAX],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());
    }

    #[test]
    fn two_refused_records_state_two_notes_each_naming_its_record() {
        let mut refusal = LaneRefusals::new();
        let short_weight_lane = || {
            PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                Some(vec![1.0]),
                false,
            )
        };
        let first = super::note_refusal(
            short_weight_lane(),
            &mut refusal,
            "consolidated_a5_03_32 at byte 64",
        );
        let second = super::note_refusal(
            short_weight_lane(),
            &mut refusal,
            "consolidated_a5_03_32 at byte 128",
        );
        assert!(first.is_none(), "the refused record states no pcurve");
        assert!(second.is_none(), "the refused record states no pcurve");
        let notes = refusal.take_notes();
        assert_eq!(notes.len(), 2, "one note per refused record: {notes:?}");
        for (note, record) in notes.iter().zip([
            "consolidated_a5_03_32 at byte 64",
            "consolidated_a5_03_32 at byte 128",
        ]) {
            assert!(
                note.message.contains("pole(s) against"),
                "the note states both lane counts: {}",
                note.message
            );
            assert!(
                note.message.contains(record),
                "the note names the record that stated the lanes: {}",
                note.message
            );
        }
        assert!(
            refusal.take_notes().is_empty(),
            "the sink is empty once its notes are taken"
        );
    }

    #[test]
    fn a_reversed_parameter_range_states_a_named_refusal() {
        let geometry = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(2.0, -1.0),
                Point2::new(3.0, 4.0),
            )
            .expect("valid LinePcurve fixture"),
        );
        let mut refusal = LaneRefusals::new();
        assert_eq!(
            reverse_pcurve_geometry(&geometry, [9.0, 5.0], &mut refusal, "e5 pcurve at byte 64"),
            None,
            "a range that does not increase is refused"
        );
        let notes = refusal.take_notes();
        assert_eq!(notes.len(), 1, "one note per refused record: {notes:?}");
        assert!(
            notes[0].message.contains("e5 pcurve at byte 64"),
            "the note names the record that stated the range: {}",
            notes[0].message
        );
        assert!(
            notes[0].message.contains("[9, 5]"),
            "the note states the refused range: {}",
            notes[0].message
        );

        let mut refusal = LaneRefusals::new();
        assert_eq!(
            reverse_curve_geometry(
                &CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0)
                            .unit()
                            .expect("nonzero fixture direction"),
                    )
                    .expect("valid LineCurve fixture"),
                )),
                [f64::NAN, 1.0],
                &mut refusal,
                "e5 curve at byte 128",
            ),
            None,
            "a non-finite range bound is refused"
        );
        let notes = refusal.take_notes();
        assert_eq!(notes.len(), 1, "one note per refused record: {notes:?}");
        assert!(
            notes[0].message.contains("e5 curve at byte 128"),
            "the note names the record that stated the range: {}",
            notes[0].message
        );
    }
}
