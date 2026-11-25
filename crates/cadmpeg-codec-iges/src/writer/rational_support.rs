// SPDX-License-Identifier: Apache-2.0
//! Analytic face supports restated as exact rational B-spline surfaces, which
//! a trimmed surface (Type 144 over Type 128) can bound.

use crate::entities::curve_conversion::{elliptical_arc_nurbs, ANGULAR_TOLERANCE};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::pcurve_uv;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

/// The document with each cylinder, cone, sphere or torus face support
/// restated as the rational B-spline surface that equals it over the domain
/// its faces' parameter curves bound, and those curves restated on it. A
/// support whose parameter curves are not all isoparametric stays analytic.
pub(super) fn rational_analytic_supports(ir: &CadIr) -> Result<Cow<'_, CadIr>, CodecError> {
    let forms = ir
        .geometry
        .surfaces
        .iter()
        .filter_map(|surface| match &surface.geometry {
            SurfaceGeometry::Solved(geometry) => {
                Some((surface.id.as_str(), Revolved::of(geometry)?))
            }
            SurfaceGeometry::Procedural { .. } => None,
        })
        .collect::<Vec<_>>();
    if forms.is_empty() {
        return Ok(Cow::Borrowed(ir));
    }
    let ends = pcurve_ends(ir)?;
    let mut surfaces = Vec::new();
    let mut pcurves = BTreeMap::new();
    for (surface, form) in forms {
        let Some(restated) = restated_support(&ends, ir, surface, &form) else {
            continue;
        };
        if restated
            .pcurves
            .keys()
            .any(|pcurve| pcurves.contains_key(pcurve))
        {
            continue;
        }
        surfaces.push((surface, restated.surface));
        pcurves.extend(restated.pcurves);
    }
    if surfaces.is_empty() {
        return Ok(Cow::Borrowed(ir));
    }
    let mut restated = ir.clone();
    for (id, nurbs) in surfaces {
        if let Some(surface) = restated
            .geometry
            .surfaces
            .iter_mut()
            .find(|surface| surface.id.as_str() == id)
        {
            surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs));
        }
    }
    for pcurve in &mut restated.geometry.pcurves {
        if let Some(nurbs) = pcurves.remove(pcurve.id.as_str()) {
            pcurve.geometry = PcurveGeometry::Nurbs { nurbs };
        }
    }
    Ok(Cow::Owned(restated))
}

/// The finite start and end points of each bounded parameter curve.
fn pcurve_ends(ir: &CadIr) -> Result<BTreeMap<&str, [Point2; 2]>, CodecError> {
    let mut ends = BTreeMap::new();
    for pcurve in &ir.geometry.pcurves {
        let Some(range) = pcurve.parameter_range() else {
            continue;
        };
        let [start, end] = range.get();
        let at = |parameter| {
            finite_or_refusal(pcurve_uv(
                EvaluationAdmission::Standard,
                &pcurve.geometry,
                parameter,
            ))
        };
        if let (Some(start), Some(end)) = (at(start)?, at(end)?) {
            ends.insert(pcurve.id.as_str(), [start.get(), end.get()]);
        }
    }
    Ok(ends)
}

/// An analytic surface as `O + ρ(v)(cos u X + k sin u Y) + h(v) Z`.
struct Revolved {
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
    profile: Profile,
}

enum Profile {
    /// `ρ(v) = radius + slope v`, `h(v) = v`, with `k = ratio`.
    Ruled { radius: f64, slope: f64, ratio: f64 },
    /// `ρ(v) = offset + radius cos v`, `h(v) = radius sin v`, with `k = 1`.
    Circular { offset: f64, radius: f64 },
}

impl Revolved {
    fn of(geometry: &SolvedSurfaceGeometry) -> Option<Self> {
        let (origin, frame, profile) = match geometry {
            SolvedSurfaceGeometry::Cylinder(cylinder) => (
                cylinder.origin(),
                cylinder.frame(),
                Profile::Ruled {
                    radius: cylinder.radius().get(),
                    slope: 0.0,
                    ratio: 1.0,
                },
            ),
            SolvedSurfaceGeometry::Cone(cone) => (
                cone.origin(),
                cone.frame(),
                Profile::Ruled {
                    radius: cone.radius().get(),
                    slope: cone.half_angle().get().portable_tan(),
                    ratio: cone.ratio().get(),
                },
            ),
            SolvedSurfaceGeometry::Sphere(sphere) => (
                sphere.center(),
                sphere.frame(),
                Profile::Circular {
                    offset: 0.0,
                    radius: sphere.radius().get(),
                },
            ),
            SolvedSurfaceGeometry::Torus(torus) => (
                torus.center(),
                torus.frame(),
                Profile::Circular {
                    offset: torus.major_radius().get(),
                    radius: torus.minor_radius().get(),
                },
            ),
            _ => return None,
        };
        let z_axis = *frame.axis().as_raw();
        let x_axis = *frame.reference().as_raw();
        Some(Self {
            origin: origin.get(),
            x_axis,
            y_axis: z_axis.cross(x_axis),
            z_axis,
            profile,
        })
    }
}

struct Restated {
    surface: NurbsSurface,
    pcurves: BTreeMap<String, PcurveNurbs>,
}

/// One isoparametric parameter curve: its parameter range and its end points.
struct Isoparametric {
    id: String,
    range: [f64; 2],
    start: Point2,
    end: Point2,
}

fn restated_support(
    ends: &BTreeMap<&str, [Point2; 2]>,
    ir: &CadIr,
    surface: &str,
    form: &Revolved,
) -> Option<Restated> {
    let geometry = &ir.geometry;
    let faces = geometry
        .faces
        .iter()
        .filter(|face| face.surface.as_str() == surface)
        .collect::<Vec<_>>();
    if faces.is_empty() {
        return None;
    }
    let mut curves = Vec::new();
    let mut u_extent = Extent::default();
    let mut v_extent = Extent::default();
    let mut seen = BTreeSet::new();
    for face in faces {
        let mut loops = face.loops.stated().peekable();
        loops.peek()?;
        for loop_id in loops {
            let loop_ = geometry.loops.iter().find(|loop_| loop_.id == *loop_id)?;
            for coedge_id in loop_.coedges() {
                let coedge = geometry
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id == *coedge_id)?;
                if coedge.pcurves.is_empty() {
                    return None;
                }
                for pcurve_use in &coedge.pcurves {
                    if !seen.insert(pcurve_use.pcurve.as_str()) {
                        continue;
                    }
                    let pcurve = geometry
                        .pcurves
                        .iter()
                        .find(|pcurve| pcurve.id == pcurve_use.pcurve)?;
                    let range = pcurve.parameter_range()?.get();
                    if let Some(used) = pcurve_use.parameter_range {
                        let used = used.endpoints();
                        if used != range && used != [range[1], range[0]] {
                            return None;
                        }
                    }
                    let [start, end] = *ends.get(pcurve.id.as_str())?;
                    let points = match &pcurve.geometry {
                        PcurveGeometry::Line(_) => vec![start, end],
                        PcurveGeometry::Nurbs { nurbs } => nurbs
                            .control_points()
                            .into_iter()
                            .map(cadmpeg_ir::units::FinitePoint2::get)
                            .collect(),
                        _ => return None,
                    };
                    let isoparametric = |coordinate: fn(&Point2) -> f64| {
                        let first = coordinate(&points[0]);
                        points
                            .iter()
                            .all(|point| (coordinate(point) - first).abs() <= ANGULAR_TOLERANCE)
                    };
                    let monotone = |coordinate: fn(&Point2) -> f64| {
                        let steps = points
                            .windows(2)
                            .map(|pair| coordinate(&pair[1]) - coordinate(&pair[0]))
                            .collect::<Vec<_>>();
                        steps.iter().all(|step| *step >= 0.0)
                            || steps.iter().all(|step| *step <= 0.0)
                    };
                    let constant_u = isoparametric(|point| point.u);
                    let constant_v = isoparametric(|point| point.v);
                    if constant_u == constant_v
                        || (constant_u && !monotone(|point| point.v))
                        || (constant_v && !monotone(|point| point.u))
                    {
                        return None;
                    }
                    for point in points.iter().chain([&start, &end]) {
                        u_extent.include(point.u);
                        v_extent.include(point.v);
                    }
                    curves.push(Isoparametric {
                        id: pcurve.id.as_str().to_owned(),
                        range,
                        start,
                        end,
                    });
                }
            }
        }
    }
    let [u_start, u_end] = u_extent.bounds()?;
    let [v_start, v_end] = v_extent.bounds()?;
    if u_end - u_start > TAU + ANGULAR_TOLERANCE {
        return None;
    }
    let u_arc = UnitArc::new([u_start, u_end])?;
    let (v_profile, v_map) = match form.profile {
        Profile::Ruled {
            radius,
            slope,
            ratio,
        } => {
            let at = |v: f64| slope.mul_add(v, radius);
            if at(v_start) <= 0.0 || at(v_end) <= 0.0 {
                return None;
            }
            (
                VProfile {
                    knots: vec![v_start, v_start, v_end, v_end],
                    points: vec![(at(v_start), v_start), (at(v_end), v_end)],
                    weights: vec![1.0, 1.0],
                    degree: 1,
                    ratio,
                },
                None,
            )
        }
        Profile::Circular { offset, radius } => {
            if radius <= 0.0 {
                return None;
            }
            let arc = UnitArc::new([v_start, v_end])?;
            (
                VProfile {
                    knots: arc.knots.clone(),
                    points: arc
                        .points
                        .iter()
                        .map(|point| (radius.mul_add(point.u, offset), radius * point.v))
                        .collect(),
                    weights: arc.weights.clone(),
                    degree: 2,
                    ratio: 1.0,
                },
                Some(arc),
            )
        }
    };
    let mut rows = Vec::new();
    let mut weights = Vec::new();
    for (direction, u_weight) in u_arc.points.iter().zip(&u_arc.weights) {
        let mut row = Vec::new();
        let mut row_weights = Vec::new();
        for ((rho, height), v_weight) in v_profile.points.iter().zip(&v_profile.weights) {
            let radial = form.x_axis.scale(rho * direction.u)
                + form.y_axis.scale(rho * v_profile.ratio * direction.v)
                + form.z_axis.scale(*height);
            row.push(Point3::new(
                form.origin.x + radial.x,
                form.origin.y + radial.y,
                form.origin.z + radial.z,
            ));
            row_weights.push(u_weight * v_weight);
        }
        rows.push(row);
        weights.push(row_weights);
    }
    let surface = NurbsSurface::new(
        NurbsSurfaceAxis::new(2, u_arc.knots.clone(), false),
        NurbsSurfaceAxis::new(v_profile.degree, v_profile.knots, false),
        NurbsPoleGrid::from_lanes(rows, Some(weights)).ok()?,
        false,
    )
    .ok()?;
    let map = |point: Point2| {
        Some(Point2::new(
            u_arc.parameter(point.u)?,
            v_map
                .as_ref()
                .map_or(Some(point.v), |arc| arc.parameter(point.v))?,
        ))
    };
    let mut pcurves = BTreeMap::new();
    for curve in curves {
        let [start, end] = curve.range;
        let nurbs = PcurveNurbs::from_lanes(
            1,
            vec![start, start, end, end],
            vec![map(curve.start)?, map(curve.end)?],
            None,
            false,
        )
        .ok()?;
        pcurves.insert(curve.id, nurbs);
    }
    Some(Restated { surface, pcurves })
}

struct VProfile {
    knots: Vec<f64>,
    /// `(ρ, h)` of each pole.
    points: Vec<(f64, f64)>,
    weights: Vec<f64>,
    degree: u32,
    ratio: f64,
}

#[derive(Default)]
struct Extent(Option<[f64; 2]>);

impl Extent {
    fn include(&mut self, value: f64) {
        let [low, high] = self.0.get_or_insert([value, value]);
        *low = low.min(value);
        *high = high.max(value);
    }

    /// The extent, when it has positive width.
    fn bounds(&self) -> Option<[f64; 2]> {
        self.0.filter(|[low, high]| high > low)
    }
}

/// The rational quadratic unit circular arc over an angle interval, with
/// knots at its segment boundaries' angles.
struct UnitArc {
    knots: Vec<f64>,
    points: Vec<Point2>,
    weights: Vec<f64>,
    boundaries: Vec<f64>,
}

impl UnitArc {
    fn new(interval: [f64; 2]) -> Option<Self> {
        let arc = elliptical_arc_nurbs(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            PositiveLength::new(1.0)?,
            PositiveLength::new(1.0)?,
            interval,
        )
        .ok()??;
        let knots = arc
            .knots()
            .finite_knots()
            .map(FiniteReal::get)
            .collect::<Vec<_>>();
        let mut boundaries = knots.clone();
        boundaries.dedup();
        Some(Self {
            points: arc
                .control_points()
                .into_iter()
                .map(|point| {
                    let point = point.get();
                    Point2::new(point.x, point.y)
                })
                .collect(),
            weights: arc
                .weights()?
                .into_iter()
                .map(cadmpeg_ir::scalar::NonZeroReal::get)
                .collect(),
            knots,
            boundaries,
        })
    }

    /// The arc parameter at the point of angle `angle`.
    fn parameter(&self, angle: f64) -> Option<f64> {
        let first = *self.boundaries.first()?;
        let last = *self.boundaries.last()?;
        if angle < first - ANGULAR_TOLERANCE || angle > last + ANGULAR_TOLERANCE {
            return None;
        }
        let angle = angle.clamp(first, last);
        let segment = self.boundaries.windows(2).find(|pair| angle <= pair[1])?;
        let [start, end] = [segment[0], segment[1]];
        let sweep = end - start;
        let offset = angle - start - sweep * 0.5;
        let fraction = (1.0 + (offset * 0.5).portable_tan() / (sweep * 0.25).portable_tan()) * 0.5;
        Some(fraction.mul_add(sweep, start))
    }
}
