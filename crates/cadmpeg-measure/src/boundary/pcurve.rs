// SPDX-License-Identifier: Apache-2.0
//! Pcurve uses of a coedge: their intervals and the witness against the
//! coedge's 3D curve.
//!
//! A single use takes the first candidate interval whose mapped endpoints
//! lie within the allowance of the coedge's traversal ends: the use's stored
//! range, the pcurve's stored range, its trim range, the edge interval and
//! its negation, the pcurve's domain, then an interval recovered by Newton
//! steps on the mapped pcurve. A closed coedge's direction comes from the
//! tangent of its 3D curve, else from the stored use range. Several uses form
//! a chain in their stated order or in reverse.
//!
//! The witness maps the pcurve image at the Gauss–Kronrod nodes of every
//! breakpoint interval onto the 3D curve. The matched curve parameters must
//! advance monotonically from one end of the 3D interval to the other.

use cadmpeg_ir::eval::decode::pcurve_uv;
use cadmpeg_ir::eval::pcurve_tangent;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::PcurveUse;

use crate::canonical::curve::CurveChart;
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::geom::combine;
use crate::owners::Owners;
use crate::quadrature::gauss_kronrod_nodes;
use crate::reason::UnavailableReason;

/// Newton steps of one pcurve endpoint recovery from one seed.
const MAX_PCURVE_RECOVERY_STEPS: usize = 32;

/// One pcurve use on its interval, in traversal order.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Span<'ir> {
    pub(crate) geometry: &'ir PcurveGeometry,
    pub(crate) t0: f64,
    pub(crate) t1: f64,
    /// The pcurve's model-space fit tolerance.
    pub(crate) fit: f64,
}

impl Span<'_> {
    /// The parameter-plane point at `t`.
    pub(crate) fn uv(&self, eval: &Eval<'_, '_>, t: f64) -> Measure<[f64; 2]> {
        let point = eval
            .work
            .evaluate(eval.ctx, |admission| pcurve_uv(admission, self.geometry, t))?
            .get();
        Ok([point.u, point.v])
    }

    /// The parameter-plane velocity at `t`.
    pub(crate) fn duv(&self, eval: &Eval<'_, '_>, t: f64) -> Measure<[f64; 2]> {
        let tangent = eval
            .work
            .evaluate(eval.ctx, |admission| {
                pcurve_tangent(admission, self.geometry, t)
            })?
            .get();
        Ok([tangent.u, tangent.v])
    }

    /// Breakpoints of the pcurve strictly inside the span.
    pub(crate) fn breaks(&self) -> Vec<f64> {
        let (low, high) = (self.t0.min(self.t1), self.t0.max(self.t1));
        breaks(self.geometry)
            .into_iter()
            .filter(|t| *t > low && *t < high)
            .collect()
    }
}

/// Parameters where a pcurve's smoothness may break: the knots of a NURBS
/// basis.
fn breaks(geometry: &PcurveGeometry) -> Vec<f64> {
    match geometry {
        PcurveGeometry::Nurbs { nurbs } => nurbs.knots().as_slice().to_vec(),
        PcurveGeometry::Transformed(placed) => breaks(placed.basis()),
        PcurveGeometry::Offset(offset) => breaks(offset.basis()),
        PcurveGeometry::Trimmed(trimmed) => breaks(trimmed.basis()),
        _ => Vec::new(),
    }
}

/// What the resolution of one coedge's pcurve uses reads.
pub(crate) struct UseFrame<'a, 'ir> {
    pub(crate) owners: &'a Owners<'ir>,
    pub(crate) chart: &'a Chart<'ir>,
    pub(crate) alpha: f64,
    /// The model points the coedge starts and ends at, in traversal order.
    pub(crate) start: Point3,
    pub(crate) end: Point3,
    /// Whether the coedge starts and ends at one vertex.
    pub(crate) closed: bool,
    /// The coedge's 3D curve and its interval in traversal order.
    pub(crate) curve: Option<(&'a CurveChart<'ir>, [f64; 2])>,
    /// The edge's stored interval, in edge order.
    pub(crate) edge_range: Option<[f64; 2]>,
}

/// The resolved uses of a coedge.
pub(crate) struct Resolved<'ir> {
    pub(crate) spans: Vec<Span<'ir>>,
    /// Whether the traversal direction is known. A closed pcurve-only use
    /// with no stored range has none.
    pub(crate) directed: bool,
}

fn failure(reason: UnavailableReason, message: &str) -> MeasureFailure {
    MeasureFailure::unavailable(reason, None, message.to_owned())
}

/// An evaluation whose unavailability is a candidate's failure, not the
/// face's: `None` for an unavailable value; a session refusal propagates.
fn soft<T>(result: Measure<T>) -> Measure<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(MeasureFailure::Refusal(limit)) => Err(MeasureFailure::Refusal(limit)),
        Err(MeasureFailure::Unavailable { .. }) => Ok(None),
    }
}

/// How a candidate interval meets the coedge's ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    /// Its start maps to the coedge's start.
    Forward,
    /// Its end maps to the coedge's start.
    Backward,
    /// A closed coedge: both ends map to its one vertex.
    Either,
}

impl Fit {
    /// The direction the fit states, if it states one.
    const fn direction(self) -> Option<bool> {
        match self {
            Self::Forward => Some(true),
            Self::Backward => Some(false),
            Self::Either => None,
        }
    }
}

/// A candidate interval and whether it is the use's own stored range.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    range: [f64; 2],
    declared: bool,
}

impl<'ir> UseFrame<'_, 'ir> {
    fn pcurve(&self, use_: &'ir PcurveUse) -> Measure<&'ir Pcurve> {
        self.owners.pcurve(use_.pcurve.as_str()).ok_or_else(|| {
            MeasureFailure::unavailable(
                UnavailableReason::BoundaryCoedgeUnresolved,
                Some(use_.pcurve.as_str()),
                "the coedge's pcurve does not resolve",
            )
        })
    }

    /// The model point of a pcurve point.
    fn map(&self, eval: &Eval<'_, '_>, uv: [f64; 2]) -> Measure<Point3> {
        Ok(self.chart.partials(eval, uv[0], uv[1])?.point)
    }

    /// The model-space velocity of the mapped pcurve at `t`.
    fn mapped_velocity(&self, eval: &Eval<'_, '_>, span: &Span<'_>, t: f64) -> Measure<Vector3> {
        let uv = span.uv(eval, t)?;
        let duv = span.duv(eval, t)?;
        let partials = self.chart.partials(eval, uv[0], uv[1])?;
        Ok(combine(&[(duv[0], partials.du), (duv[1], partials.dv)]))
    }

    /// Resolve the uses of a coedge. With `direction_optional`, a closed use
    /// whose direction nothing decides resolves undirected instead of
    /// failing.
    pub(crate) fn resolve(
        &self,
        eval: &Eval<'_, '_>,
        uses: &'ir [PcurveUse],
        direction_optional: bool,
    ) -> Measure<Resolved<'ir>> {
        match uses {
            [] => Err(failure(
                UnavailableReason::BoundaryCoedgeUnresolved,
                "the coedge has no pcurve",
            )),
            [use_] => self.single(eval, use_, direction_optional),
            _ => self.chain(eval, uses),
        }
    }

    fn single(
        &self,
        eval: &Eval<'_, '_>,
        use_: &'ir PcurveUse,
        direction_optional: bool,
    ) -> Measure<Resolved<'ir>> {
        let pcurve = self.pcurve(use_)?;
        let fit = pcurve
            .fit_tolerance()
            .map_or(0.0, cadmpeg_ir::geometry::FitTolerance::get);
        let span = |[t0, t1]: [f64; 2]| Span {
            geometry: &pcurve.geometry,
            t0,
            t1,
            fit,
        };
        let mut candidates: Vec<Candidate> = Vec::new();
        if let Some(range) = use_.parameter_range {
            candidates.push(Candidate {
                range: range.endpoints(),
                declared: true,
            });
        }
        if let Some(range) = pcurve.parameter_range() {
            candidates.push(Candidate {
                range: range.get(),
                declared: false,
            });
        }
        if let Some(range) = pcurve.geometry.trim_range(eval.ctx)? {
            candidates.push(Candidate {
                range,
                declared: false,
            });
        }
        if let Some([start, end]) = self.edge_range {
            for range in [[start, end], [-start, -end]] {
                candidates.push(Candidate {
                    range,
                    declared: false,
                });
            }
        }
        if let Some(domain) = pcurve.geometry.parameter_domain(eval.ctx)? {
            candidates.push(Candidate {
                range: domain.endpoints(),
                declared: false,
            });
        }
        let mut qualified: Vec<(Candidate, Option<bool>)> = Vec::new();
        for candidate in &candidates {
            if let Some(fit) = self.qualify(eval, &span(candidate.range))? {
                qualified.push((*candidate, fit.direction()));
            }
        }
        if qualified.is_empty() {
            if let Some(range) = self.recover(eval, &span([0.0, 0.0]), &candidates)? {
                if let Some(fit) = self.qualify(eval, &span(range))? {
                    qualified.push((
                        Candidate {
                            range,
                            declared: false,
                        },
                        fit.direction(),
                    ));
                }
            }
        }
        let Some(&(chosen, direction)) = qualified
            .iter()
            .find(|(candidate, _)| candidate.declared)
            .or_else(|| qualified.first())
        else {
            return Err(failure(
                UnavailableReason::BoundaryInversionFailed,
                "no pcurve interval maps onto the coedge's vertices",
            ));
        };
        let chosen = if chosen.declared {
            chosen
        } else {
            self.disambiguate(eval, &span, chosen, &qualified)?
        };
        let forward = match direction {
            Some(forward) => Some(forward),
            None => self.closed_direction(eval, &span(chosen.range), chosen.declared)?,
        };
        let [a, b] = chosen.range;
        match forward {
            Some(true) => Ok(Resolved {
                spans: vec![span([a, b])],
                directed: true,
            }),
            Some(false) => Ok(Resolved {
                spans: vec![span([b, a])],
                directed: true,
            }),
            None if direction_optional => Ok(Resolved {
                spans: vec![span([a, b])],
                directed: false,
            }),
            None => Err(failure(
                UnavailableReason::BoundaryPcurveDirectionAmbiguous,
                "a closed pcurve-only coedge has no direction",
            )),
        }
    }

    /// How a span meets the coedge's ends, or `None` when it does not.
    fn qualify(&self, eval: &Eval<'_, '_>, span: &Span<'_>) -> Measure<Option<Fit>> {
        if span.t0 == span.t1 {
            return Ok(None);
        }
        let (Some(a), Some(b)) = (soft(span.uv(eval, span.t0))?, soft(span.uv(eval, span.t1))?)
        else {
            return Ok(None);
        };
        let (Some(a), Some(b)) = (soft(self.map(eval, a))?, soft(self.map(eval, b))?) else {
            return Ok(None);
        };
        let near = |p: Point3, q: Point3| p.distance(q) <= self.alpha;
        let forward = near(a, self.start) && near(b, self.end);
        let backward = near(b, self.start) && near(a, self.end);
        Ok(match (forward, backward) {
            (true, true) if self.closed => Some(Fit::Either),
            (true, _) => Some(Fit::Forward),
            (_, true) => Some(Fit::Backward),
            _ => None,
        })
    }

    /// Among qualifying candidates whose images differ, the one the 3D
    /// curve's midpoint picks.
    fn disambiguate(
        &self,
        eval: &Eval<'_, '_>,
        span: &impl Fn([f64; 2]) -> Span<'ir>,
        chosen: Candidate,
        qualified: &[(Candidate, Option<bool>)],
    ) -> Measure<Candidate> {
        let midpoint = |candidate: &Candidate| -> Measure<Point3> {
            let [a, b] = candidate.range;
            self.map(eval, span(candidate.range).uv(eval, a.midpoint(b))?)
        };
        let reference = midpoint(&chosen)?;
        let mut differs = false;
        for (candidate, _) in qualified {
            if midpoint(candidate)?.distance(reference) > self.alpha {
                differs = true;
            }
        }
        if !differs {
            return Ok(chosen);
        }
        let Some((curve, [c0, c1])) = self.curve else {
            return Err(failure(
                UnavailableReason::BoundaryPcurveIntervalAmbiguous,
                "two pcurve intervals qualify with different images",
            ));
        };
        let target = curve.point(eval, c0.midpoint(c1))?;
        let mut best = (f64::INFINITY, chosen);
        for (candidate, _) in qualified {
            let distance = midpoint(candidate)?.distance(target);
            if distance < best.0 {
                best = (distance, *candidate);
            }
        }
        Ok(best.1)
    }

    /// The direction of a closed span: the 3D tangent at the traversal
    /// start, else the stored use range.
    fn closed_direction(
        &self,
        eval: &Eval<'_, '_>,
        span: &Span<'_>,
        declared: bool,
    ) -> Measure<Option<bool>> {
        if let Some((curve, [c0, c1])) = self.curve {
            let sign = if c1 >= c0 { 1.0 } else { -1.0 };
            let tangent = curve.tangent(eval, c0)?.scale(sign);
            let image_sign = if span.t1 >= span.t0 { 1.0 } else { -1.0 };
            let image = self.mapped_velocity(eval, span, span.t0)?.scale(image_sign);
            let product = image.dot(tangent);
            if product > 0.0 {
                return Ok(Some(true));
            }
            if product < 0.0 {
                return Ok(Some(false));
            }
        }
        Ok(declared.then_some(true))
    }

    /// An interval recovered by Newton steps on the mapped pcurve toward each
    /// traversal end.
    fn recover(
        &self,
        eval: &Eval<'_, '_>,
        template: &Span<'ir>,
        candidates: &[Candidate],
    ) -> Measure<Option<[f64; 2]>> {
        let mut seeds: Vec<f64> = candidates
            .iter()
            .flat_map(|candidate| candidate.range)
            .collect();
        if let Some([low, high]) = candidates.last().map(|candidate| candidate.range) {
            seeds.push(low.midpoint(high));
        }
        if seeds.is_empty() {
            seeds.push(0.0);
        }
        let start = self.solve(eval, template, self.start, &seeds)?;
        let end = self.solve(eval, template, self.end, &seeds)?;
        Ok(match (start, end) {
            (Some(a), Some(b)) if a != b => Some([a, b]),
            _ => None,
        })
    }

    /// The pcurve parameter whose mapped point is `target`, from the first
    /// seed that converges within the allowance.
    fn solve(
        &self,
        eval: &Eval<'_, '_>,
        template: &Span<'ir>,
        target: Point3,
        seeds: &[f64],
    ) -> Measure<Option<f64>> {
        for seed in seeds {
            let mut t = *seed;
            for _ in 0..MAX_PCURVE_RECOVERY_STEPS {
                let Some(uv) = soft(template.uv(eval, t))? else {
                    break;
                };
                let Some(point) = soft(self.map(eval, uv))? else {
                    break;
                };
                let gap = target.vector_from(point);
                if gap.norm() <= self.alpha {
                    return Ok(Some(t));
                }
                let Some(velocity) = soft(self.mapped_velocity(eval, template, t))? else {
                    break;
                };
                let speed = velocity.dot(velocity);
                if !(speed.is_finite() && speed > 0.0) {
                    break;
                }
                let step = gap.dot(velocity) / speed;
                if !step.is_finite() || step == 0.0 {
                    break;
                }
                t += step;
            }
        }
        Ok(None)
    }

    /// A chain of uses, each on its own stored range, in stated order or in
    /// reverse.
    fn chain(&self, eval: &Eval<'_, '_>, uses: &'ir [PcurveUse]) -> Measure<Resolved<'ir>> {
        let mut spans = Vec::with_capacity(uses.len());
        for use_ in uses {
            let pcurve = self.pcurve(use_)?;
            let range = match use_
                .parameter_range
                .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints)
                .or_else(|| {
                    pcurve
                        .parameter_range()
                        .map(cadmpeg_ir::units::FiniteVector::get)
                }) {
                Some(range) => Some(range),
                None => pcurve.geometry.trim_range(eval.ctx)?,
            };
            let range = match range {
                Some(range) => range,
                None => match pcurve.geometry.parameter_domain(eval.ctx)? {
                    Some(domain) => domain.endpoints(),
                    None => {
                        return Err(failure(
                            UnavailableReason::BoundaryPcurveChainBroken,
                            "a pcurve use of the chain has no interval",
                        ))
                    }
                },
            };
            spans.push(Span {
                geometry: &pcurve.geometry,
                t0: range[0],
                t1: range[1],
                fit: pcurve
                    .fit_tolerance()
                    .map_or(0.0, cadmpeg_ir::geometry::FitTolerance::get),
            });
        }
        let reversed: Vec<Span<'ir>> = spans
            .iter()
            .rev()
            .map(|span| Span {
                t0: span.t1,
                t1: span.t0,
                ..*span
            })
            .collect();
        let forward = self.joins(eval, &spans)?;
        let backward = self.joins(eval, &reversed)?;
        let use_forward = match (forward, backward) {
            (true, true) => match spans.first() {
                Some(first) => self.closed_direction(eval, first, true)?.unwrap_or(true),
                None => true,
            },
            (true, false) => true,
            (false, true) => false,
            (false, false) => {
                return Err(failure(
                    UnavailableReason::BoundaryPcurveChainBroken,
                    "the coedge's pcurve uses do not join from its start to its end",
                ))
            }
        };
        Ok(Resolved {
            spans: if use_forward { spans } else { reversed },
            directed: true,
        })
    }

    /// Whether spans run from the traversal start to its end and join within
    /// the allowance.
    fn joins(&self, eval: &Eval<'_, '_>, spans: &[Span<'_>]) -> Measure<bool> {
        let mut previous = self.start;
        for span in spans {
            let (Some(a), Some(b)) = (soft(span.uv(eval, span.t0))?, soft(span.uv(eval, span.t1))?)
            else {
                return Ok(false);
            };
            let (Some(a), Some(b)) = (soft(self.map(eval, a))?, soft(self.map(eval, b))?) else {
                return Ok(false);
            };
            if a.distance(previous) > self.alpha {
                return Ok(false);
            }
            previous = b;
        }
        Ok(previous.distance(self.end) <= self.alpha)
    }

    /// The witness of a pcurve image against the 3D curve: the largest
    /// accepted distance, or `boundary.pcurve-curve-mismatch`.
    pub(crate) fn witness(&self, eval: &Eval<'_, '_>, spans: &[Span<'_>]) -> Measure<f64> {
        let Some((curve, [c0, c1])) = self.curve else {
            return Ok(0.0);
        };
        let delta = spans.iter().map(|span| span.fit).fold(self.alpha, f64::max);
        let (Some(first), Some((last_index, last))) =
            (spans.first(), spans.iter().enumerate().next_back())
        else {
            return Ok(0.0);
        };
        let mut nodes: Vec<(usize, f64)> = vec![(0, first.t0)];
        for (index, span) in spans.iter().enumerate() {
            let mut cuts = vec![span.t0];
            let mut inner = span.breaks();
            if span.t1 < span.t0 {
                inner.reverse();
            } else {
                inner.sort_by(f64::total_cmp);
            }
            cuts.extend(inner);
            cuts.push(span.t1);
            for pair in cuts.windows(2) {
                for t in gauss_kronrod_nodes(pair[0], pair[1]) {
                    nodes.push((index, t));
                }
            }
        }
        nodes.push((last_index, last.t1));
        let period = curve.period();
        let direction = if c1 >= c0 { 1.0 } else { -1.0 };
        let mismatch =
            |message: &str| failure(UnavailableReason::BoundaryPcurveCurveMismatch, message);
        let mut previous = c0;
        let mut farthest: f64 = 0.0;
        for (position, (index, t)) in nodes.iter().enumerate() {
            let point = self.map(eval, spans[*index].uv(eval, *t)?)?;
            let Some(found) = curve.parameter(eval, point, previous, delta)? else {
                return Err(mismatch("the pcurve image leaves the edge's 3D curve"));
            };
            let lifted = match period {
                Some(period) => found + ((previous - found) / period).round() * period,
                None => found,
            };
            let reached = curve.point(eval, lifted)?;
            farthest = farthest.max(reached.distance(point));
            let speed = curve.tangent(eval, lifted)?.norm();
            let resolution = if speed > 0.0 { delta / speed } else { 0.0 };
            if position == 0 && (lifted - c0).abs() > resolution {
                return Err(mismatch(
                    "the pcurve image does not start at the edge's 3D interval",
                ));
            }
            if position > 0 && (lifted - previous) * direction < -resolution {
                return Err(mismatch(
                    "the pcurve image runs back along the edge's 3D curve",
                ));
            }
            previous = lifted;
        }
        let speed = curve.tangent(eval, c1)?.norm();
        let resolution = if speed > 0.0 { delta / speed } else { 0.0 };
        if (previous - c1).abs() > resolution {
            return Err(mismatch(
                "the pcurve image does not cover the edge's 3D interval",
            ));
        }
        Ok(farthest)
    }
}

/// Whether the chart's region rule orients a lone loop by its role.
pub(crate) fn nesting_domain(chart: &Chart) -> bool {
    matches!(chart, Chart::Plane(_) | Chart::Cylinder(_) | Chart::Cone(_))
}
