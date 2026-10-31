// SPDX-License-Identifier: Apache-2.0
//! Solved caches for constant-radius rolling-ball blends.
//!
//! A `00 38` blend states its two supports, its spine and its radius, and no
//! surface ([spec §7.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#74-constant-radius-rolling-ball-surfaces)).
//! The cache sweeps the ball's circular section along the spine between the
//! two contact points: a rational surface, quadratic across the section and
//! cubic along the spine. Its natural normal points away from the ball centre.

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis, WeightedPole3};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::NonZeroReal;

use super::evaluation;

/// Patches a cache may hold along its spine.
const MAX_PATCHES: usize = 512;
/// Patches the first fit uses along its spine.
const SEED_PATCHES: usize = 4;
/// Spine samples that bracket the projection of a boundary point.
const SPINE_SAMPLES: usize = 256;
/// Golden-section steps that refine a projection onto the spine.
const PROJECTION_STEPS: usize = 80;
/// Points sampled on each boundary edge.
const EDGE_SAMPLES: usize = 32;
/// Largest contact-distance error of a section, as a share of the radius.
const CONTACT_RELATIVE_TOLERANCE: f64 = 1.0e-3;
/// Contact-distance error, as a share of the radius, at which a centre is solved.
const CENTER_RELATIVE_TOLERANCE: f64 = 1.0e-10;
/// Newton steps that move a stored spine point to the ball centre.
const CENTER_STEPS: usize = 16;
/// Section angle, in radians, short of a half turn at which an arc is refused.
const MIN_SECTION_ANGLE: f64 = 1.0e-6;
/// Central-difference step of the spine tangent, as a share of the extent.
const TANGENT_STEP: f64 = 1.0e-6;
/// Largest gap between boundary parameters, as a share of a closed spine's
/// period, that leaves the whole period covered.
const RING_GAP_SHARE: f64 = 0.125;
/// Share of the projected stretch by which each end of it is widened.
const EXTENT_MARGIN: f64 = 0.05;
/// Spine-parameter fractions at which a patch interpolates its sections.
const NODE_FRACTIONS: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
/// Spine-parameter fractions at which a patch is measured between its nodes.
const PROBE_U: [f64; 3] = [1.0 / 6.0, 0.5, 5.0 / 6.0];
/// Section-parameter fractions at which a patch is measured.
const PROBE_V: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
/// Distance in millimetres, beyond the cache's fit tolerance, at which a
/// face vertex is off the cache: the linear resolution, `1e-8` m.
const VERTEX_ALLOWANCE: f64 = 1.0e-5;
/// Section-parameter seeds of the search for a vertex's foot on the cache.
const VERTEX_SEEDS_V: [f64; 3] = [0.0, 0.5, 1.0];

type Vec3 = [f64; 3];
type Homogeneous = [f64; 4];

/// What a blend states about its surface, in model millimetres.
pub(super) struct BlendInputs<'a> {
    /// Locus of the ball centre.
    pub(super) spine: &'a CurveGeometry,
    /// The two support surfaces, in stored order.
    pub(super) supports: [&'a SolvedSurfaceGeometry; 2],
    /// Ball radius.
    pub(super) radius: f64,
    /// Points on the boundary of the face the blend carries.
    pub(super) boundary: &'a [Point3],
    /// The face's vertices, which lie on the blend.
    pub(super) vertices: &'a [Point3],
    /// Largest distance the cache may add to the blend's own data.
    pub(super) tolerance: f64,
}

/// A solved cache and its largest distance from the blend.
pub(super) struct BlendCache {
    pub(super) surface: NurbsSurface,
    pub(super) fit_tolerance: f64,
}

/// Points along one boundary edge: its parameter range, or the whole period
/// of a closed conic edge that states no range.
pub(super) fn edge_samples(
    ctx: &DecodeContext<'_>,
    curve: &CurveGeometry,
    range: Option<[f64; 2]>,
) -> Result<Vec<Point3>, CodecError> {
    let range = match (range, curve) {
        (Some(range), _) => range,
        (
            None,
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)),
        ) => [0.0, TAU],
        (None, _) => return Ok(Vec::new()),
    };
    let mut points = Vec::with_capacity(EDGE_SAMPLES + 1);
    for index in 0..=EDGE_SAMPLES {
        let fraction = fraction_of(index, EDGE_SAMPLES);
        if let Some(point) = evaluation::curve_point(ctx, curve, lerp(range, fraction))? {
            points.push(point.get());
        }
    }
    Ok(points)
}

/// Solve the cache of one blend over the stretch of spine its face covers.
/// `None` when the blend's data does not define a section along that
/// stretch, when no fit within `MAX_PATCHES` reaches the tolerance, or when
/// a face vertex is off the cache.
pub(super) fn solve(
    ctx: &DecodeContext<'_>,
    inputs: &BlendInputs<'_>,
) -> Result<Option<BlendCache>, CodecError> {
    if !(inputs.radius.is_finite() && inputs.radius > 0.0) || inputs.boundary.is_empty() {
        return Ok(None);
    }
    let Some(spine) = Spine::new(ctx, inputs.spine)? else {
        return Ok(None);
    };
    let mut parameters = Vec::with_capacity(inputs.boundary.len());
    for point in inputs.boundary {
        let Some(parameter) = spine.project(ctx, to3(*point))? else {
            return Ok(None);
        };
        parameters.push(parameter);
    }
    let Some(extent) = spine.extent(&parameters) else {
        return Ok(None);
    };
    // Boundary points are projected onto the stored spine, which can stop
    // short of where a narrowing face ends on the exact centre locus. The
    // widened stretch covers that end; it falls back to the projected
    // stretch where the ball does not reach both supports beyond it.
    let widened = spine.widen(extent);
    for stretch in [widened, extent] {
        if let Some(cache) = solve_over(ctx, inputs, &spine, stretch)? {
            let holds = passes_through(ctx, &cache, &spine, stretch, inputs.vertices)?;
            return Ok(holds.then_some(cache));
        }
        if widened == extent {
            break;
        }
    }
    Ok(None)
}

/// The largest distance of a vertex from `cache` over the stretch of
/// `spine` the vertices project to, or `None` when one does not project.
///
/// # Errors
///
/// Propagates an evaluator's resource refusal.
pub(super) fn vertex_miss(
    ctx: &DecodeContext<'_>,
    cache: &BlendCache,
    spine: &CurveGeometry,
    vertices: &[Point3],
) -> Result<Option<f64>, CodecError> {
    let Some(spine) = Spine::new(ctx, spine)? else {
        return Ok(None);
    };
    let mut parameters = Vec::with_capacity(vertices.len());
    for vertex in vertices {
        let Some(parameter) = spine.project(ctx, to3(*vertex))? else {
            return Ok(None);
        };
        parameters.push(parameter);
    }
    let Some(extent) = spine.extent(&parameters) else {
        return Ok(None);
    };
    largest_miss(ctx, cache, &spine, extent, vertices).map(Some)
}

/// Whether every vertex lies on the cache within its fit tolerance and the
/// linear resolution. Sections can agree with one another and still miss
/// the face, as when a support search settles on another branch; the
/// vertices are exact points of the blend and catch that.
fn passes_through(
    ctx: &DecodeContext<'_>,
    cache: &BlendCache,
    spine: &Spine<'_>,
    extent: [f64; 2],
    vertices: &[Point3],
) -> Result<bool, CodecError> {
    let miss = largest_miss(ctx, cache, spine, extent, vertices)?;
    Ok(miss <= cache.fit_tolerance + VERTEX_ALLOWANCE)
}

/// The largest distance of a vertex from `cache`.
fn largest_miss(
    ctx: &DecodeContext<'_>,
    cache: &BlendCache,
    spine: &Spine<'_>,
    extent: [f64; 2],
    vertices: &[Point3],
) -> Result<f64, CodecError> {
    let mut largest: f64 = 0.0;
    for vertex in vertices {
        let point = to3(*vertex);
        let along = spine
            .project(ctx, point)?
            .map_or(lerp(extent, 0.5), |parameter| {
                spine.within(parameter, extent)
            });
        let seeds = VERTEX_SEEDS_V
            .map(|across| Some(Point2::new(along, across)))
            .into_iter()
            .chain([None]);
        let mut nearest = f64::INFINITY;
        for seed in seeds {
            let Some(parameters) =
                evaluation::nurbs_surface_parameter_near_point(ctx, &cache.surface, *vertex, seed)?
            else {
                continue;
            };
            let parameters = parameters.get();
            if let Some(foot) =
                evaluation::nurbs_surface_point(ctx, &cache.surface, parameters.u, parameters.v)?
            {
                nearest = nearest.min(norm(sub(to3(foot.get()), point)));
            }
        }
        largest = largest.max(nearest);
    }
    Ok(largest)
}

/// Fit the cache over one stretch of spine.
fn solve_over(
    ctx: &DecodeContext<'_>,
    inputs: &BlendInputs<'_>,
    spine: &Spine<'_>,
    extent: [f64; 2],
) -> Result<Option<BlendCache>, CodecError> {
    let mut solver = Solver {
        ctx,
        inputs,
        supports: inputs
            .supports
            .map(|support| SurfaceGeometry::Solved(support.clone())),
        spine,
        extent,
        sections: BTreeMap::new(),
    };
    let mut pending = solver.seed_ranges(extent);
    let mut accepted: Vec<Patch> = Vec::new();
    while let Some(range) = pending.pop() {
        if accepted.len() + pending.len() >= MAX_PATCHES {
            return Ok(None);
        }
        let Some(patch) = solver.fit(range)? else {
            return Ok(None);
        };
        if patch.error <= inputs.tolerance {
            accepted.push(patch);
            continue;
        }
        let middle = lerp(range, 0.5);
        if !(middle > range[0] && middle < range[1]) {
            return Ok(None);
        }
        pending.push([middle, range[1]]);
        pending.push([range[0], middle]);
    }
    accepted.sort_by(|left, right| left.range[0].total_cmp(&right.range[0]));
    let fit_error = accepted.iter().map(|patch| patch.error).fold(0.0, f64::max);
    let deviation = solver
        .sections
        .values()
        .map(|section| section.deviation)
        .fold(0.0, f64::max);
    Ok(build_surface(&accepted).map(|surface| BlendCache {
        surface,
        fit_tolerance: fit_error + deviation,
    }))
}

/// The parameter domain a spine is projected over.
#[derive(Clone, Copy)]
enum SpineDomain {
    /// An unbounded line, projected in closed form.
    Line,
    /// A bounded open curve.
    Bounded([f64; 2]),
    /// A closed curve over one period.
    Periodic([f64; 2]),
}

struct Spine<'a> {
    curve: &'a CurveGeometry,
    domain: SpineDomain,
    /// Interior knots of a NURBS spine, where its derivatives can jump.
    knots: Vec<f64>,
}

impl<'a> Spine<'a> {
    fn new(ctx: &DecodeContext<'_>, curve: &'a CurveGeometry) -> Result<Option<Self>, CodecError> {
        let CurveGeometry::Solved(solved) = curve else {
            return Ok(None);
        };
        let (domain, knots) = match solved {
            SolvedCurveGeometry::Line(_) => (SpineDomain::Line, Vec::new()),
            SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
                (SpineDomain::Periodic([0.0, TAU]), Vec::new())
            }
            SolvedCurveGeometry::Nurbs(nurbs) => {
                let knots = nurbs.knots().as_slice();
                let Ok(degree) = usize::try_from(nurbs.degree()) else {
                    return Ok(None);
                };
                let (Some(&first), Some(&last)) =
                    (knots.get(degree), knots.get(nurbs.pole_count()))
                else {
                    return Ok(None);
                };
                if first >= last {
                    return Ok(None);
                }
                let interior = knots
                    .iter()
                    .copied()
                    .filter(|knot| *knot > first && *knot < last)
                    .collect::<Vec<_>>();
                let ends = (
                    evaluation::curve_point(ctx, curve, first)?,
                    evaluation::curve_point(ctx, curve, last)?,
                );
                let (Some(start), Some(end)) = ends else {
                    return Ok(None);
                };
                let gap = norm(sub(to3(start.get()), to3(end.get())));
                let scale = norm(to3(start.get())).max(1.0);
                let domain = if nurbs.periodic() || gap <= scale * f64::EPSILON * 1024.0 {
                    SpineDomain::Periodic([first, last])
                } else {
                    SpineDomain::Bounded([first, last])
                };
                (domain, interior)
            }
            _ => return Ok(None),
        };
        Ok(Some(Self {
            curve,
            domain,
            knots,
        }))
    }

    fn point(&self, ctx: &DecodeContext<'_>, parameter: f64) -> Result<Option<Vec3>, CodecError> {
        let parameter = match self.domain {
            SpineDomain::Periodic([first, last]) => {
                first + (parameter - first).rem_euclid(last - first)
            }
            SpineDomain::Line | SpineDomain::Bounded(_) => parameter,
        };
        Ok(evaluation::curve_point(ctx, self.curve, parameter)?.map(|point| to3(point.get())))
    }

    /// The spine parameter closest to `point`.
    fn project(&self, ctx: &DecodeContext<'_>, point: Vec3) -> Result<Option<f64>, CodecError> {
        let [first, last] = match self.domain {
            SpineDomain::Line => {
                let (Some(origin), Some(unit)) = (self.point(ctx, 0.0)?, self.point(ctx, 1.0)?)
                else {
                    return Ok(None);
                };
                let direction = sub(unit, origin);
                let length = dot(direction, direction);
                if length <= 0.0 {
                    return Ok(None);
                }
                return Ok(Some(dot(sub(point, origin), direction) / length));
            }
            SpineDomain::Bounded(range) | SpineDomain::Periodic(range) => range,
        };
        let mut best: Option<(usize, f64)> = None;
        for index in 0..=SPINE_SAMPLES {
            let parameter = lerp([first, last], fraction_of(index, SPINE_SAMPLES));
            let Some(sample) = self.point(ctx, parameter)? else {
                continue;
            };
            let distance = norm(sub(sample, point));
            if best.is_none_or(|(_, nearest)| distance < nearest) {
                best = Some((index, distance));
            }
        }
        let Some((index, _)) = best else {
            return Ok(None);
        };
        let step = (last - first) / f64_from_index(SPINE_SAMPLES).unwrap_or(f64::NAN);
        let centre = lerp([first, last], fraction_of(index, SPINE_SAMPLES));
        let (mut low, mut high) = (centre - step, centre + step);
        if let SpineDomain::Bounded(_) = self.domain {
            low = low.max(first);
            high = high.min(last);
        }
        let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
        for _ in 0..PROJECTION_STEPS {
            let left = high - ratio * (high - low);
            let right = low + ratio * (high - low);
            let (Some(at_left), Some(at_right)) = (self.point(ctx, left)?, self.point(ctx, right)?)
            else {
                return Ok(None);
            };
            if norm(sub(at_left, point)) <= norm(sub(at_right, point)) {
                high = right;
            } else {
                low = left;
            }
        }
        let parameter = 0.5 * (low + high);
        Ok(Some(match self.domain {
            SpineDomain::Periodic([first, last]) => {
                first + (parameter - first).rem_euclid(last - first)
            }
            SpineDomain::Line | SpineDomain::Bounded(_) => parameter,
        }))
    }

    /// A spine parameter moved into `extent`: by whole periods on a closed
    /// spine, then held to the extent's ends.
    fn within(&self, parameter: f64, extent: [f64; 2]) -> f64 {
        let parameter = match self.domain {
            SpineDomain::Periodic([first, last]) => {
                extent[0] + (parameter - extent[0]).rem_euclid(last - first)
            }
            SpineDomain::Line | SpineDomain::Bounded(_) => parameter,
        };
        parameter.clamp(extent[0], extent[1])
    }

    /// The stretch widened by `EXTENT_MARGIN` at each end, inside a bounded
    /// spine's domain; on a closed spine, no more than one period.
    fn widen(&self, [low, high]: [f64; 2]) -> [f64; 2] {
        let margin = (high - low) * EXTENT_MARGIN;
        match self.domain {
            SpineDomain::Line => [low - margin, high + margin],
            SpineDomain::Bounded([first, last]) => {
                [(low - margin).max(first), (high + margin).min(last)]
            }
            SpineDomain::Periodic([first, last]) => {
                let period = last - first;
                if high - low + 2.0 * margin >= period {
                    [low, low + period]
                } else {
                    [low - margin, high + margin]
                }
            }
        }
    }

    /// The stretch of spine the boundary parameters span. On a closed spine
    /// it is the period less the largest gap between them, or the whole
    /// period when no gap is large.
    fn extent(&self, parameters: &[f64]) -> Option<[f64; 2]> {
        let mut sorted = parameters.to_vec();
        sorted.sort_by(f64::total_cmp);
        let (&low, &high) = (sorted.first()?, sorted.last()?);
        let extent = match self.domain {
            SpineDomain::Line | SpineDomain::Bounded(_) => [low, high],
            SpineDomain::Periodic([first, last]) => {
                let period = last - first;
                let mut gap = (low + period - high, high, low + period);
                for pair in sorted.windows(2) {
                    if let [before, after] = *pair {
                        if after - before > gap.0 {
                            gap = (after - before, before, after);
                        }
                    }
                }
                if gap.0 <= period * RING_GAP_SHARE {
                    [gap.2, gap.2 + period]
                } else {
                    let end = if gap.1 < gap.2 { gap.1 + period } else { gap.1 };
                    [gap.2, end]
                }
            }
        };
        (extent[0] < extent[1]).then_some(extent)
    }
}

/// One circular section: the ball centre and its two contact points.
struct Section {
    center: Vec3,
    /// The contacts in arc order: the arc from the first to the second turns
    /// against the spine tangent, so the cache normal points away from the
    /// centre. Where the contacts meet, either order is the same point.
    contacts: [Vec3; 2],
    /// Support parameters by support, which seed the next section.
    seeds: [Option<Point2>; 2],
    /// Largest difference between a contact distance and the radius.
    deviation: f64,
}

struct Patch {
    range: [f64; 2],
    /// Bézier poles: four along the spine, three across the section.
    poles: [[Homogeneous; 3]; 4],
    error: f64,
}

struct Solver<'a, 'c> {
    ctx: &'a DecodeContext<'c>,
    inputs: &'a BlendInputs<'a>,
    supports: [SurfaceGeometry; 2],
    spine: &'a Spine<'a>,
    extent: [f64; 2],
    /// Solved sections by the bits of their spine parameter. The ordered map
    /// makes the nearest-section seed, and so the decode, deterministic.
    sections: BTreeMap<u64, Section>,
}

impl Solver<'_, '_> {
    fn section(&mut self, parameter: f64) -> Result<Option<&Section>, CodecError> {
        let key = parameter.to_bits();
        if !self.sections.contains_key(&key) {
            let Some(section) = self.solve_section(parameter)? else {
                return Ok(None);
            };
            self.sections.insert(key, section);
        }
        Ok(self.sections.get(&key))
    }

    /// The section at a spine parameter. The stored spine can be a chart of
    /// the centre locus, so the centre moves from the stored point, in the
    /// plane normal to the spine, to the point at the radius from both
    /// supports (Newton steps on the two support distances).
    fn solve_section(&self, parameter: f64) -> Result<Option<Section>, CodecError> {
        let (Some(stored), Some(tangent)) = (
            self.spine.point(self.ctx, parameter)?,
            self.tangent(parameter)?,
        ) else {
            return Ok(None);
        };
        let mut seeds = self
            .sections
            .iter()
            .min_by(|left, right| {
                let distance = |key: u64| (f64::from_bits(key) - parameter).abs();
                distance(*left.0).total_cmp(&distance(*right.0))
            })
            .map_or([None, None], |(_, section)| section.seeds);
        let radius = self.inputs.radius;
        let mut center = stored;
        let mut best: Option<Section> = None;
        for _ in 0..CENTER_STEPS {
            let mut contacts = [[0.0; 3]; 2];
            let mut residuals = [0.0; 2];
            for (side, support) in self.supports.iter().enumerate() {
                let Some((contact, seed)) = foot(self.ctx, support, center, seeds[side])? else {
                    return Ok(None);
                };
                contacts[side] = contact;
                seeds[side] = seed;
                residuals[side] = radius - norm(sub(center, contact));
            }
            let deviation = residuals[0].abs().max(residuals[1].abs());
            if best
                .as_ref()
                .is_none_or(|section| deviation < section.deviation)
            {
                best = Some(Section {
                    center,
                    contacts,
                    seeds,
                    deviation,
                });
            }
            if deviation <= radius * CENTER_RELATIVE_TOLERANCE {
                break;
            }
            let (Some(first), Some(second)) = (
                unit(sub(center, contacts[0])),
                unit(sub(center, contacts[1])),
            ) else {
                return Ok(None);
            };
            let determinant = dot(first, cross(second, tangent));
            if determinant.abs() <= f64::EPSILON || !determinant.is_finite() {
                return Ok(None);
            }
            let step = scale(
                add(
                    scale(cross(second, tangent), residuals[0]),
                    scale(cross(tangent, first), residuals[1]),
                ),
                determinant.recip(),
            );
            center = add(center, step);
        }
        Ok(best
            .filter(|section| {
                section.deviation <= radius * CONTACT_RELATIVE_TOLERANCE
                    && norm(sub(section.center, stored)) <= radius
            })
            .map(|mut section| {
                let first = sub(section.contacts[0], section.center);
                let second = sub(section.contacts[1], section.center);
                if dot(cross(first, second), tangent) > 0.0 {
                    section.contacts.swap(0, 1);
                }
                section
            }))
    }

    /// The unit spine tangent by a central difference inside the extent.
    fn tangent(&self, parameter: f64) -> Result<Option<Vec3>, CodecError> {
        let step = (self.extent[1] - self.extent[0]) * TANGENT_STEP;
        let (Some(before), Some(after)) = (
            self.spine
                .point(self.ctx, (parameter - step).max(self.extent[0]))?,
            self.spine
                .point(self.ctx, (parameter + step).min(self.extent[1]))?,
        ) else {
            return Ok(None);
        };
        Ok(unit(sub(after, before)))
    }

    /// Initial patch ranges: the extent cut at every interior spine knot
    /// and into at least `SEED_PATCHES` pieces.
    fn seed_ranges(&self, extent: [f64; 2]) -> Vec<[f64; 2]> {
        let mut breaks = vec![extent[0], extent[1]];
        let period = match self.spine.domain {
            SpineDomain::Periodic([first, last]) => Some(last - first),
            SpineDomain::Line | SpineDomain::Bounded(_) => None,
        };
        for &knot in &self.spine.knots {
            for shift in [-1.0, 0.0, 1.0, 2.0] {
                let candidate = knot + period.map_or(0.0, |period| shift * period);
                if candidate > extent[0] && candidate < extent[1] {
                    breaks.push(candidate);
                }
            }
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        let pieces = SEED_PATCHES.div_ceil(breaks.len() - 1).max(1);
        let mut ranges = Vec::new();
        for pair in breaks.windows(2) {
            if let [low, high] = *pair {
                for piece in 0..pieces {
                    ranges.push([
                        lerp([low, high], fraction_of(piece, pieces)),
                        lerp([low, high], fraction_of(piece + 1, pieces)),
                    ]);
                }
            }
        }
        ranges.reverse();
        ranges
    }

    fn fit(&mut self, range: [f64; 2]) -> Result<Option<Patch>, CodecError> {
        let mut nodes = [[[0.0; 4]; 3]; 4];
        for (node, fraction) in nodes.iter_mut().zip(NODE_FRACTIONS) {
            let Some(section) = self.section(lerp(range, fraction))? else {
                return Ok(None);
            };
            let Some(poles) = section_poles(section) else {
                return Ok(None);
            };
            *node = poles;
        }
        let mut poles = [[[0.0; 4]; 3]; 4];
        for column in 0..3 {
            let lane = [
                nodes[0][column],
                nodes[1][column],
                nodes[2][column],
                nodes[3][column],
            ];
            let bezier = interpolating_bezier(lane);
            for (row, pole) in bezier.into_iter().enumerate() {
                if pole[3] <= 0.0 {
                    return Ok(None);
                }
                poles[row][column] = pole;
            }
        }
        let mut error = 0.0_f64;
        for fraction in PROBE_U {
            let Some(section) = self.section(lerp(range, fraction))? else {
                return Ok(None);
            };
            for across in PROBE_V {
                let Some(point) = patch_point(&poles, fraction, across) else {
                    return Ok(None);
                };
                error = error.max(arc_distance(point, section));
            }
        }
        Ok(Some(Patch {
            range,
            poles,
            error,
        }))
    }
}

/// The closest point of a support to the ball centre, and the support
/// parameters that seed the next search on a NURBS support.
fn foot(
    ctx: &DecodeContext<'_>,
    support: &SurfaceGeometry,
    center: Vec3,
    seed: Option<Point2>,
) -> Result<Option<(Vec3, Option<Point2>)>, CodecError> {
    let SurfaceGeometry::Solved(solved) = support else {
        return Ok(None);
    };
    let point = Point3::new(center[0], center[1], center[2]);
    let parameters = match solved {
        SolvedSurfaceGeometry::Plane(_)
        | SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_) => {
            cadmpeg_ir::eval::analytic_surface_parameters_solved(solved, point)
                .map(cadmpeg_ir::units::FinitePoint2::get)
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            if cone.ratio().get() != 1.0 {
                return Ok(None);
            }
            let origin = to3(cone.origin().get());
            let axis = vector(*cone.frame().axis().as_raw());
            let reference = vector(*cone.frame().reference().as_raw());
            let offset = sub(center, origin);
            let along = dot(offset, axis);
            let across = dot(offset, reference);
            let transverse = dot(offset, cross(axis, reference));
            let slope = cone.half_angle().get().tan();
            let radius = cone.radius().get();
            let height =
                (along + slope * (across.hypot(transverse) - radius)) / (1.0 + slope * slope);
            if radius + height * slope <= 0.0 {
                return Ok(None);
            }
            Some(Point2::new(transverse.atan2(across), height))
        }
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            let Some(parameters) =
                evaluation::nurbs_surface_parameter_near_point(ctx, nurbs, point, seed)?
            else {
                return Ok(None);
            };
            let parameters = parameters.get();
            return Ok(
                evaluation::nurbs_surface_point(ctx, nurbs, parameters.u, parameters.v)?
                    .map(|contact| (to3(contact.get()), Some(parameters))),
            );
        }
        _ => None,
    };
    let Some(parameters) = parameters else {
        return Ok(None);
    };
    Ok(
        evaluation::surface_point(ctx, support, parameters.u, parameters.v)?
            .map(|contact| (to3(contact.get()), None)),
    )
}

/// The rational quadratic poles of a section's arc, from its first contact
/// to its second.
fn section_poles(section: &Section) -> Option<[Homogeneous; 3]> {
    let [start, end] = section.contacts;
    let first = sub(start, section.center);
    let second = sub(end, section.center);
    let (first_length, second_length) = (norm(first), norm(second));
    let angle = norm(cross(first, second)).atan2(dot(first, second));
    if !(angle.is_finite() && angle <= PI - MIN_SECTION_ANGLE) {
        return None;
    }
    let weight = (0.5 * angle).cos();
    let bisector = unit(add(
        scale(first, first_length.recip()),
        scale(second, second_length.recip()),
    ))?;
    let radius = 0.5 * (first_length + second_length);
    let middle = add(section.center, scale(bisector, radius / weight));
    Some([
        homogeneous(start, 1.0),
        homogeneous(middle, weight),
        homogeneous(end, 1.0),
    ])
}

/// Bézier poles of the cubic through four values at fractions 0, 1/3, 2/3
/// and 1.
fn interpolating_bezier(values: [Homogeneous; 4]) -> [Homogeneous; 4] {
    let combine = |weights: [f64; 4]| {
        let mut pole = [0.0; 4];
        for (value, weight) in values.iter().zip(weights) {
            for (lane, component) in pole.iter_mut().zip(value) {
                *lane += weight * component;
            }
        }
        pole
    };
    [
        values[0],
        combine([-5.0 / 6.0, 3.0, -1.5, 1.0 / 3.0]),
        combine([1.0 / 3.0, -1.5, 3.0, -5.0 / 6.0]),
        values[3],
    ]
}

fn patch_point(poles: &[[Homogeneous; 3]; 4], along: f64, across: f64) -> Option<Vec3> {
    let rest = 1.0 - along;
    let cubic = [
        rest * rest * rest,
        3.0 * rest * rest * along,
        3.0 * rest * along * along,
        along * along * along,
    ];
    let spare = 1.0 - across;
    let quadratic = [spare * spare, 2.0 * spare * across, across * across];
    let mut sum = [0.0; 4];
    for (row, row_weight) in poles.iter().zip(cubic) {
        for (pole, column_weight) in row.iter().zip(quadratic) {
            for (lane, component) in sum.iter_mut().zip(pole) {
                *lane += row_weight * column_weight * component;
            }
        }
    }
    (sum[3] > 0.0).then(|| [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]])
}

/// Distance from `point` to a section's arc.
fn arc_distance(point: Vec3, section: &Section) -> f64 {
    let [start, end] = section.contacts;
    let first = sub(start, section.center);
    let second = sub(end, section.center);
    let ends = norm(sub(point, start)).min(norm(sub(point, end)));
    let (Some(normal), Some(first_unit)) = (unit(cross(first, second)), unit(first)) else {
        return ends;
    };
    let offset = sub(point, section.center);
    let height = dot(offset, normal);
    let planar = sub(offset, scale(normal, height));
    let angle = norm(cross(first, second)).atan2(dot(first, second));
    let position = dot(cross(first_unit, planar), normal).atan2(dot(first_unit, planar));
    if !(0.0..=angle).contains(&position) {
        return ends;
    }
    let radius = 0.5 * (norm(first) + norm(second));
    height.hypot(norm(planar) - radius)
}

fn build_surface(patches: &[Patch]) -> Option<NurbsSurface> {
    let first = patches.first()?;
    let mut knots = vec![first.range[0]; 4];
    let mut rows = Vec::with_capacity(3 * patches.len() + 1);
    for (index, patch) in patches.iter().enumerate() {
        let start = usize::from(index > 0);
        for pole_row in &patch.poles[start..] {
            let mut row = Vec::with_capacity(3);
            for pole in pole_row {
                row.push(WeightedPole3 {
                    point: Point3::new(pole[0] / pole[3], pole[1] / pole[3], pole[2] / pole[3]),
                    weight: NonZeroReal::new(pole[3])?,
                });
            }
            rows.push(row);
        }
        knots.extend([patch.range[1]; 3]);
    }
    knots.push(*knots.last()?);
    NurbsSurface::new(
        NurbsSurfaceAxis::new(3, knots, false),
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsPoleGrid::Rational { rows },
        false,
    )
    .ok()
}

fn fraction_of(index: usize, count: usize) -> f64 {
    match (f64_from_index(index), f64_from_index(count)) {
        (Some(index), Some(count)) if count > 0.0 => index / count,
        _ => f64::NAN,
    }
}

fn lerp(range: [f64; 2], fraction: f64) -> f64 {
    range[0] * (1.0 - fraction) + range[1] * fraction
}

fn to3(point: Point3) -> Vec3 {
    [point.x, point.y, point.z]
}

fn vector(vector: cadmpeg_ir::math::Vector3) -> Vec3 {
    [vector.x, vector.y, vector.z]
}

fn homogeneous(point: Vec3, weight: f64) -> Homogeneous {
    [
        point[0] * weight,
        point[1] * weight,
        point[2] * weight,
        weight,
    ]
}

fn add(left: Vec3, right: Vec3) -> Vec3 {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn sub(left: Vec3, right: Vec3) -> Vec3 {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(vector: Vec3, factor: f64) -> Vec3 {
    [vector[0] * factor, vector[1] * factor, vector[2] * factor]
}

fn dot(left: Vec3, right: Vec3) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: Vec3, right: Vec3) -> Vec3 {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn norm(vector: Vec3) -> f64 {
    dot(vector, vector).sqrt()
}

fn unit(vector: Vec3) -> Option<Vec3> {
    let length = norm(vector);
    (length > 0.0 && length.is_finite()).then(|| scale(vector, length.recip()))
}

#[cfg(test)]
mod tests;
