// SPDX-License-Identifier: Apache-2.0
//! Prepared inverse queries for one curve witness.

mod boundary;
mod speed;

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::nurbs_curve_point_at;
use cadmpeg_ir::eval::{
    curve_tangent_solved, finite_or_refusal, nurbs_curve_parameter_domain,
    nurbs_curve_parameter_near_point,
};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::bounds::speed_bound_by;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeLength};
use cadmpeg_ir::topology::{IncreasingParameterInterval, ParameterInterval};

use boundary::BoundaryIndex;
use speed::FiniteSpeed;

const MAX_CURVE_PARAMETER_INTERVALS: usize = 512;
const MAX_CURVE_PARAMETER_NEWTON_ITERATIONS: usize = 12;

const MAX_CACHED_CURVE_POINTS: usize = 1 << 12;

#[derive(Default)]
struct CurvePoints {
    values: HashMap<u64, Option<FinitePoint3>>,
}

impl CurvePoints {
    fn point<E>(
        &mut self,
        parameter: FiniteReal,
        read: impl FnOnce() -> Result<Option<FinitePoint3>, E>,
    ) -> Result<Option<FinitePoint3>, E> {
        let key = parameter.get().to_bits();
        if let Some(point) = self.values.get(&key) {
            return Ok(*point);
        }
        let point = read()?;
        if self.values.len() < MAX_CACHED_CURVE_POINTS && self.values.try_reserve(1).is_ok() {
            self.values.insert(key, point);
        }
        Ok(point)
    }
}

pub(crate) struct CurveInverter<'curve> {
    geometry: &'curve SolvedCurveGeometry,
    curve: &'curve NurbsCurve,
    points: CurvePoints,
    finite_speed: Option<FiniteSpeed<'curve>>,
    boundary_index: Option<BoundaryIndex>,
    indexed_cache_size: usize,
}

enum Search {
    Witness(Option<FiniteReal>),
    Stateless,
}

impl<'curve> CurveInverter<'curve> {
    pub(crate) fn new(geometry: &'curve SolvedCurveGeometry) -> Option<Self> {
        let SolvedCurveGeometry::Nurbs(curve) = geometry else {
            return None;
        };
        Some(Self {
            geometry,
            curve,
            points: CurvePoints::default(),
            finite_speed: FiniteSpeed::new(curve),
            boundary_index: None,
            indexed_cache_size: usize::MAX,
        })
    }

    pub(crate) const fn curve(&self) -> &'curve NurbsCurve {
        self.curve
    }

    fn position(
        &mut self,
        ctx: &DecodeContext<'_>,
        parameter: FiniteReal,
    ) -> Result<Option<FinitePoint3>, ResourceLimit> {
        let curve = self.curve;
        self.points.point(parameter, || {
            finite_or_refusal(nurbs_curve_point_at(ctx, curve, parameter.get()))
        })
    }

    pub(crate) fn parameter(
        &mut self,
        ctx: &DecodeContext<'_>,
        point: Point3,
        tolerance: f64,
        seed: f64,
    ) -> Result<Option<FiniteReal>, CodecError> {
        let Some(tolerance) = NonNegativeLength::new(tolerance) else {
            return Ok(None);
        };
        let Some(seed) = FiniteReal::new(seed) else {
            return Ok(None);
        };
        let result = {
            let _depth = ctx.enter_nested("IR NURBS curve inversion depth")?;
            let result = self.search(ctx, point, tolerance.get(), seed);
            if let Some(limit) = ctx.resource_refusal() {
                return Err(limit.into());
            }
            result?
        };
        match result {
            Search::Witness(parameter) => Ok(parameter),
            Search::Stateless => nurbs_curve_parameter_near_point(
                ctx,
                self.curve,
                point,
                tolerance.get(),
                seed.get(),
            ),
        }
    }

    fn search(
        &mut self,
        ctx: &DecodeContext<'_>,
        point: Point3,
        tolerance: f64,
        seed: FiniteReal,
    ) -> Result<Search, ResourceLimit> {
        let curve = self.curve;
        let Some(degree) = usize::try_from(curve.degree()).ok() else {
            return Ok(Search::Witness(None));
        };
        let count = curve.pole_count();
        let Some(domain) = nurbs_curve_parameter_domain(curve).map(ParameterInterval::from) else {
            return Ok(Search::Witness(None));
        };
        if degree == 0 || !point.is_finite() {
            return Ok(Search::Witness(None));
        }
        let seed = project(domain, seed);
        if matches!(curve.pole_rows(), NurbsPoles3::Polynomial { .. })
            && self
                .position(ctx, seed)?
                .is_some_and(|position| position.distance(point) <= tolerance)
        {
            return Ok(Search::Witness(Some(seed)));
        }
        let speed_bound = if self
            .finite_speed
            .as_ref()
            .is_some_and(|speed| speed.admits(point))
        {
            None
        } else {
            let Some(bound) = speed_bound_at(curve, point) else {
                return Ok(Search::Witness(None));
            };
            Some(bound)
        };
        let mut boundaries = Vec::new();
        for index in degree..=count {
            let Some(boundary) = curve.knots().finite_knot(index) else {
                return Ok(Search::Witness(None));
            };
            boundaries.push(boundary);
        }
        if self.boundary_index.is_none() && self.indexed_cache_size != self.points.values.len() {
            self.indexed_cache_size = self.points.values.len();
            self.boundary_index = BoundaryIndex::new(&boundaries, |parameter| {
                self.points
                    .values
                    .get(&parameter.get().to_bits())
                    .copied()
                    .flatten()
            });
        }
        let boundary_witness = if let Some(index) = &self.boundary_index {
            index
                .witness(point, seed, tolerance)
                .map_or(BoundaryWitness::NoMatch, BoundaryWitness::Found)
        } else {
            nearest_boundary_witness(&boundaries, seed, tolerance, |parameter| {
                Ok(self
                    .position(ctx, parameter)?
                    .map(|position| position.distance(point)))
            })?
        };
        match boundary_witness {
            BoundaryWitness::Found(parameter) => return Ok(Search::Witness(Some(parameter))),
            BoundaryWitness::Invalid => return Ok(Search::Witness(None)),
            BoundaryWitness::NoMatch => {}
        }
        match self.newton(ctx, point, tolerance, seed, domain, &boundaries)? {
            Search::Witness(Some(parameter)) => return Ok(Search::Witness(Some(parameter))),
            Search::Stateless => return Ok(Search::Stateless),
            Search::Witness(None) => {}
        }
        let Some(speed_bound) = speed_bound.or_else(|| speed_bound_at(curve, point)) else {
            return Ok(Search::Witness(None));
        };
        let mut intervals = bounded_nearest_intervals(&boundaries, seed);
        let mut examined = 0usize;
        while let Some([start, end]) = intervals.pop() {
            examined += 1;
            if examined > MAX_CURVE_PARAMETER_INTERVALS {
                return Ok(Search::Witness(None));
            }
            let middle =
                FiniteReal::new(start.get().midpoint(end.get())).expect("finite interval midpoint");
            let Some(middle_distance) = self
                .position(ctx, middle)?
                .map(|position| position.distance(point))
            else {
                return Ok(Search::Witness(None));
            };
            if middle_distance <= tolerance {
                return Ok(Search::Witness(Some(middle)));
            }
            let (start_value, middle_value, end_value) = (start.get(), middle.get(), end.get());
            if middle_distance
                - speed_bound * (end_value - middle_value).max(middle_value - start_value)
                > tolerance
                || middle == start
                || middle == end
            {
                continue;
            }
            let halves = [[start, middle], [middle, end]];
            let nearer = usize::from(
                interval_distance_to_parameter(halves[1], seed)
                    < interval_distance_to_parameter(halves[0], seed),
            );
            intervals.push(halves[1 - nearer]);
            intervals.push(halves[nearer]);
        }
        Ok(Search::Witness(None))
    }

    fn newton(
        &mut self,
        ctx: &DecodeContext<'_>,
        point: Point3,
        tolerance: f64,
        seed: FiniteReal,
        domain: ParameterInterval,
        boundaries: &[FiniteReal],
    ) -> Result<Search, ResourceLimit> {
        let window = parameter_interval_containing(boundaries, seed).unwrap_or(domain);
        let mut parameter = project(window, seed);
        let upper = domain.endpoints()[1];
        for _ in 0..MAX_CURVE_PARAMETER_NEWTON_ITERATIONS {
            let Some(position) = self.position(ctx, parameter)? else {
                return Ok(Search::Witness(None));
            };
            let residual = Vector3::new(
                position.x - point.x,
                position.y - point.y,
                position.z - point.z,
            );
            if residual.norm() <= tolerance {
                return Ok(Search::Witness(Some(parameter)));
            }
            if self.curve.periodic() && parameter.get() == upper {
                return Ok(Search::Stateless);
            }
            let Some(tangent) = finite_or_refusal(curve_tangent_solved(
                EvaluationAdmission::Standard,
                self.geometry,
                parameter.get(),
            ))?
            else {
                return Ok(Search::Witness(None));
            };
            let tangent = tangent.get();
            let denominator = tangent.dot(tangent);
            if !denominator.is_finite() || denominator <= 0.0 {
                return Ok(Search::Witness(None));
            }
            let Some(next) = FiniteReal::new(parameter.get() - residual.dot(tangent) / denominator)
            else {
                return Ok(Search::Witness(None));
            };
            let next = project(window, next);
            if next == parameter {
                return Ok(Search::Witness(None));
            }
            parameter = next;
        }
        Ok(Search::Witness(None))
    }
}

fn speed_bound_at(curve: &NurbsCurve, point: Point3) -> Option<f64> {
    speed_bound_by(
        curve.degree(),
        curve.knots(),
        curve.pole_count(),
        |index| {
            curve
                .pole_rows()
                .point_at(index)
                .map(|point| point.get().into())
        },
        |index| curve.pole_rows().weight_at(index).unwrap_or(1.0),
        point.into(),
    )
    .map(FiniteReal::get)
}

fn project(interval: ParameterInterval, value: FiniteReal) -> FiniteReal {
    let [lower, upper] = interval.finite_endpoints();
    if value < lower {
        lower
    } else if value > upper {
        upper
    } else {
        value
    }
}

fn interval_distance_to_parameter(interval: [FiniteReal; 2], parameter: FiniteReal) -> f64 {
    let [lower, upper] = interval.map(FiniteReal::get);
    let parameter = parameter.get();
    if parameter < lower {
        lower - parameter
    } else if parameter > upper {
        parameter - upper
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug)]
struct SearchInterval {
    bounds: [FiniteReal; 2],
    distance: f64,
}

impl PartialEq for SearchInterval {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for SearchInterval {}

impl PartialOrd for SearchInterval {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchInterval {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then_with(|| self.bounds[0].get().total_cmp(&other.bounds[0].get()))
            .then_with(|| self.bounds[1].get().total_cmp(&other.bounds[1].get()))
    }
}

fn bounded_nearest_intervals(boundaries: &[FiniteReal], seed: FiniteReal) -> Vec<[FiniteReal; 2]> {
    let mut nearest = BinaryHeap::new();
    for pair in boundaries.windows(2) {
        if pair[0] >= pair[1] {
            continue;
        }
        let candidate = SearchInterval {
            bounds: [pair[0], pair[1]],
            distance: interval_distance_to_parameter([pair[0], pair[1]], seed),
        };
        if nearest.len() < MAX_CURVE_PARAMETER_INTERVALS {
            nearest.push(candidate);
        } else if let Some(mut farthest) = nearest.peek_mut() {
            if candidate < *farthest {
                *farthest = candidate;
            }
        }
    }
    let mut result = Vec::new();
    while let Some(interval) = nearest.pop() {
        result.push(interval.bounds);
    }
    result
}
#[derive(Debug, PartialEq)]
enum BoundaryWitness {
    Invalid,
    NoMatch,
    Found(FiniteReal),
}

fn nearest_boundary_witness<F>(
    boundaries: &[FiniteReal],
    seed: FiniteReal,
    tolerance: f64,
    mut distance: F,
) -> Result<BoundaryWitness, ResourceLimit>
where
    F: FnMut(FiniteReal) -> Result<Option<f64>, ResourceLimit>,
{
    let mut previous_boundary = None;
    let mut nearest = None;
    let mut nearest_seed_distance = f64::INFINITY;
    for parameter in boundaries {
        let parameter = *parameter;
        if previous_boundary == Some(parameter) {
            continue;
        }
        previous_boundary = Some(parameter);
        let seed_distance = (parameter.get() - seed.get()).abs();
        if seed_distance >= nearest_seed_distance {
            continue;
        }
        let Some(candidate_distance) = distance(parameter)? else {
            return Ok(BoundaryWitness::Invalid);
        };
        if candidate_distance <= tolerance {
            nearest = Some(parameter);
            nearest_seed_distance = seed_distance;
        }
    }
    Ok(nearest.map_or(BoundaryWitness::NoMatch, BoundaryWitness::Found))
}

fn parameter_interval_containing(
    boundaries: &[FiniteReal],
    parameter: FiniteReal,
) -> Option<ParameterInterval> {
    for pair in boundaries.windows(2) {
        if let Some(interval) = IncreasingParameterInterval::between(pair[0], pair[1])
            .filter(|_| parameter >= pair[0] && parameter <= pair[1])
            .map(ParameterInterval::from)
        {
            return Some(interval);
        }
    }
    None
}

#[cfg(test)]
mod tests;
