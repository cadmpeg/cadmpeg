// SPDX-License-Identifier: Apache-2.0
//! Carriers of tolerant edges. A tolerant edge states no curve; each of its
//! fins states a trimmed `SP_CURVE`, a parameter-space B-curve on the support
//! surface of the fin's face. The edge's model-space carrier is that curve
//! evaluated through the support.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::units::FinitePoint2;
use cadmpeg_parasolid::geometry::nurbs::BCurve;
use cadmpeg_parasolid::records::node_type as kind;
use cadmpeg_parasolid::records::primary::Node;

use crate::geometry::{self, Nodes};

const MM_PER_METRE: f64 = cadmpeg_ir::scalar::Magnification::MILLIMETERS_PER_METER.get();

/// Largest distance, millimetres, between the sampled polyline and the
/// evaluated curve at a segment midpoint, when the document states a looser
/// linear tolerance than the sampling needs. The caller's target is the
/// smaller of this and half the document tolerance. Tolerant edges also
/// admit this distance between an endpoint and the sampled curve.
pub(crate) const FIT_TOLERANCE_MM: f64 = 1.0e-4;

/// Segments sampled per knot span before midpoint refinement.
const SEED_SEGMENTS_PER_SPAN: usize = 4;

/// Midpoint bisections applied to one seed segment. A segment still
/// outside the fit tolerance at this depth is kept, and the sampled fit
/// tolerance states its midpoint departure.
const MAX_DEPTH: u32 = 12;

/// The fewest seed segments.
const MIN_SEED_SEGMENTS: usize = 8;

/// The model-space polyline of a tolerant edge's `SP_CURVE`.
pub(crate) struct Sampled {
    pub(crate) curve: NurbsCurve,
    /// Largest distance, millimetres, from an edge endpoint to the sampled
    /// curve that the trim admits.
    pub(crate) fit_tolerance: f64,
    /// Largest midpoint departure of the polyline from the evaluated curve,
    /// millimetres.
    pub(crate) departure: f64,
}

/// The `SP_CURVE` node a tolerant edge's fins state, the index of the node
/// that states it, and the parameter range its trimmed curve restricts it to.
pub(crate) fn locate<'a>(
    nodes: &Nodes<'a>,
    first_fin: Node<'a>,
) -> Option<(Node<'a>, u32, Option<[f64; 2]>)> {
    let other = first_fin
        .pointer("other")
        .filter(|index| *index > 1)
        .and_then(|index| nodes.get(&index).copied());
    [Some(first_fin), other]
        .into_iter()
        .flatten()
        .find_map(|fin| {
            let stated = nodes.get(&fin.pointer("curve").filter(|index| *index > 1)?)?;
            let basis = geometry::trim_basis(nodes, *stated);
            if basis.kind() != kind::SP_CURVE {
                return None;
            }
            let range = (stated.kind() == kind::TRIMMED_CURVE)
                .then(|| Some([stated.real("parm_1")?, stated.real("parm_2")?]))
                .flatten()
                .map(|[a, b]| [a.min(b), a.max(b)])
                .filter(|[low, high]| low.is_finite() && high.is_finite() && low < high);
            Some((basis, stated.index(), range))
        })
}

/// A support's stored parameters as the IR's: planes state metres on both
/// axes, cylinders and cones an angle and a distance in metres.
fn ir_parameters(surface: &SolvedSurfaceGeometry, uv: Point2) -> Option<FinitePoint2> {
    let point = match surface {
        SolvedSurfaceGeometry::Plane(_) => Point2::new(uv.u * MM_PER_METRE, uv.v * MM_PER_METRE),
        SolvedSurfaceGeometry::Cylinder(_) | SolvedSurfaceGeometry::Cone(_) => {
            Point2::new(uv.u, uv.v * MM_PER_METRE)
        }
        SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_)
        | SolvedSurfaceGeometry::Nurbs(_) => uv,
        _ => return None,
    };
    FinitePoint2::new(point)
}

/// The model-space polyline of the `SP_CURVE` node `sp`, restricted to
/// `range` of its parameter when stated. Absent when the support or the
/// parameter-space curve does not resolve.
pub(crate) fn sample(
    ctx: &DecodeContext<'_>,
    nodes: &Nodes<'_>,
    sp: Node<'_>,
    range: Option<[f64; 2]>,
    target: f64,
) -> Result<Option<Sampled>, CodecError> {
    let Some(support) = sp
        .pointer("surface")
        .and_then(|index| nodes.get(&index).copied())
        .and_then(|node| geometry::surface(nodes, node))
    else {
        return Ok(None);
    };
    let SurfaceGeometry::Solved(support) = support else {
        return Ok(None);
    };
    let Some(BCurve::Planar(stored)) = sp
        .pointer("b_curve")
        .and_then(|index| nodes.get(&index).copied())
        .and_then(|node| geometry::b_curve(nodes, node))
    else {
        return Ok(None);
    };
    let mut pcurve = stored;
    let mapped = pcurve
        .try_map_control_points_in_place(|point| ir_parameters(&support, point.get()).ok_or(()));
    if mapped.is_err() {
        return Ok(None);
    }
    polyline(ctx, &support, &pcurve, range, target)
}

/// Samples `pcurve`, in the IR's parameters of `support`, over `range` (its
/// knot domain when absent) and bisects every segment whose midpoint departs
/// from its chord by more than `target`.
fn polyline(
    ctx: &DecodeContext<'_>,
    support: &SolvedSurfaceGeometry,
    pcurve: &PcurveNurbs,
    range: Option<[f64; 2]>,
    target: f64,
) -> Result<Option<Sampled>, CodecError> {
    let knots = pcurve.knots();
    let Ok(degree) = usize::try_from(pcurve.degree()) else {
        return Ok(None);
    };
    let domain = knots
        .get(degree)
        .zip(
            knots
                .len()
                .checked_sub(degree + 1)
                .and_then(|index| knots.get(index)),
        )
        .map(|(low, high)| [*low, *high]);
    let Some([low, high]) = domain.map(|[low, high]| {
        range.map_or([low, high], |[start, end]| [start.max(low), end.min(high)])
    }) else {
        return Ok(None);
    };
    if !(low.is_finite() && high.is_finite() && low < high) {
        return Ok(None);
    }
    let spans = knots
        .windows(2)
        .filter(|pair| pair[0] < pair[1] && pair[0] < high && pair[1] > low)
        .count();
    let seeds = (spans * SEED_SEGMENTS_PER_SPAN).max(MIN_SEED_SEGMENTS);
    let geometry = PcurveGeometry::Nurbs {
        nurbs: pcurve.clone(),
    };
    let point_at = |parameter: f64| -> Result<Option<Point3>, CodecError> {
        let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
            ctx, &geometry, parameter,
        ))?
        else {
            return Ok(None);
        };
        let uv = uv.get();
        let solved = cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::decode::surface_point_solved(ctx, support, uv.u, uv.v),
        )?;
        Ok(solved.map(FinitePoint3::get))
    };
    let mut parameters = Vec::with_capacity(seeds + 1);
    let mut points = Vec::with_capacity(seeds + 1);
    for step in 0..=seeds {
        let (Ok(step_count), Ok(seed_count)) = (u32::try_from(step), u32::try_from(seeds)) else {
            return Ok(None);
        };
        let fraction = f64::from(step_count) / f64::from(seed_count);
        let parameter = if step == seeds {
            high
        } else {
            low + (high - low) * fraction
        };
        let Some(point) = point_at(parameter)? else {
            return Ok(None);
        };
        parameters.push(parameter);
        points.push(point);
    }
    let mut fit_tolerance = FIT_TOLERANCE_MM;
    let mut departure = 0.0_f64;
    let mut refined_parameters = vec![parameters[0]];
    let mut refined_points = vec![points[0]];
    for index in 1..parameters.len() {
        let mut pending = vec![(
            parameters[index - 1],
            points[index - 1],
            parameters[index],
            points[index],
            0_u32,
        )];
        while let Some((start, from, end, to, depth)) = pending.pop() {
            let middle = 0.5 * (start + end);
            let midpoint = if middle > start && middle < end {
                point_at(middle)?
            } else {
                None
            };
            let sagitta = midpoint.map(|point| {
                let chord = Point3::new(
                    0.5 * (from.x + to.x),
                    0.5 * (from.y + to.y),
                    0.5 * (from.z + to.z),
                );
                (point, point.distance(chord))
            });
            let split = match sagitta {
                Some((point, distance)) if distance > target => {
                    if depth < MAX_DEPTH {
                        Some(point)
                    } else {
                        // A segment at the bisection cap is kept; the stated
                        // tolerance covers its departure.
                        fit_tolerance = fit_tolerance.max(distance);
                        departure = departure.max(distance);
                        None
                    }
                }
                Some((_, distance)) => {
                    departure = departure.max(distance);
                    None
                }
                None => None,
            };
            match split {
                Some(point) => {
                    pending.push((middle, point, end, to, depth + 1));
                    pending.push((start, from, middle, point, depth + 1));
                }
                None => {
                    refined_parameters.push(end);
                    refined_points.push(to);
                }
            }
        }
    }
    let last = refined_points.len() - 1;
    let knot_count = refined_points.len() + 2;
    let mut knot_values = Vec::with_capacity(knot_count);
    knot_values.push(refined_parameters[0]);
    knot_values.extend_from_slice(&refined_parameters);
    knot_values.push(refined_parameters[last]);
    let closed = refined_points[0] == refined_points[last];
    let Ok(curve) = NurbsCurve::from_lanes(1, knot_values, refined_points, None, closed) else {
        return Ok(None);
    };
    Ok(Some(Sampled {
        curve,
        fit_tolerance,
        departure,
    }))
}

#[cfg(test)]
mod tests {
    use super::{ir_parameters, polyline, FIT_TOLERANCE_MM};
    const TARGET_MM: f64 = 5.0e-6;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::units::FinitePoint2;

    const EPS_POINT: f64 = 1.0e-9;

    fn line(from: [f64; 2], to: [f64; 2]) -> PcurveNurbs {
        PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                FinitePoint2::new(Point2::new(from[0], from[1])).expect("finite"),
                FinitePoint2::new(Point2::new(to[0], to[1])).expect("finite"),
            ],
            None,
            false,
        )
        .expect("pcurve")
    }

    fn plane() -> SolvedSurfaceGeometry {
        SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 5.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        )
    }

    fn cylinder() -> SolvedSurfaceGeometry {
        SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                10.0,
            )
            .expect("cylinder"),
        )
    }

    fn map(surface: &SolvedSurfaceGeometry, mut pcurve: PcurveNurbs) -> PcurveNurbs {
        pcurve
            .try_map_control_points_in_place(|point| ir_parameters(surface, point.get()).ok_or(()))
            .expect("mapped");
        pcurve
    }

    #[test]
    fn plane_pcurve_in_metres_samples_to_millimetres() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0; 4], &arena, &policy).expect("context");
        let surface = plane();
        let pcurve = map(&surface, line([0.0, 0.0], [0.001, 0.002]));
        let sampled = polyline(&ctx, &surface, &pcurve, None, FIT_TOLERANCE_MM)
            .expect("evaluation")
            .expect("polyline");
        let points = sampled.curve.control_points();
        let first = points[0].get();
        let last = points[points.len() - 1].get();
        for (actual, expected) in [first.x, first.y, first.z, last.x, last.y, last.z]
            .into_iter()
            .zip([0.0, 0.0, 5.0, 1.0, 2.0, 5.0])
        {
            assert!((actual - expected).abs() < EPS_POINT);
        }
        ctx.finish_session().expect("session");
    }

    #[test]
    fn cylinder_arc_is_bisected_within_the_fit_tolerance() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0; 4], &arena, &policy).expect("context");
        let surface = cylinder();
        let quarter = std::f64::consts::FRAC_PI_2;
        let pcurve = map(&surface, line([0.0, 0.002], [quarter, 0.002]));
        let sampled = polyline(
            &ctx,
            &surface,
            &pcurve,
            Some([0.25, 0.75]),
            FIT_TOLERANCE_MM,
        )
        .expect("evaluation")
        .expect("polyline");
        let points: Vec<_> = sampled
            .curve
            .control_points()
            .iter()
            .map(|point| point.get())
            .collect();
        assert!(points.len() > 9);
        for point in &points {
            assert!((point.x.hypot(point.y) - 10.0).abs() < EPS_POINT);
            assert!((point.z - 2.0).abs() < EPS_POINT);
        }
        for pair in points.windows(2) {
            let sag = 10.0 - (0.5 * (pair[0].x + pair[1].x)).hypot(0.5 * (pair[0].y + pair[1].y));
            assert!(sag <= FIT_TOLERANCE_MM);
        }
        ctx.finish_session().expect("session");
    }

    #[test]
    fn cylinder_arc_meets_a_document_scale_target() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0; 4], &arena, &policy).expect("context");
        let surface = cylinder();
        let quarter = std::f64::consts::FRAC_PI_2;
        let pcurve = map(&surface, line([0.0, 0.002], [quarter, 0.002]));
        let sampled = polyline(&ctx, &surface, &pcurve, None, TARGET_MM)
            .expect("evaluation")
            .expect("polyline");
        let coarse = polyline(&ctx, &surface, &pcurve, None, FIT_TOLERANCE_MM)
            .expect("evaluation")
            .expect("polyline");
        assert!(sampled.curve.control_points().len() > coarse.curve.control_points().len());
        assert!(sampled.departure <= TARGET_MM);
        assert!(coarse.departure <= FIT_TOLERANCE_MM);
        assert!(coarse.departure > TARGET_MM);
        ctx.finish_session().expect("session");
    }
}
