// SPDX-License-Identifier: Apache-2.0
//! Certified offset-cache fit and offset-surface parameter inversion.

use super::blend::{blend_surface_parameters_for_fit_with_grid_and_budget, BlendParameterGrid};
use super::geometry_work::GeometryWorkBudget;
#[cfg(test)]
use super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK;
use super::support_uv::{linear_knots, missing_support_parameter};
use crate::native::vector::{cross_vector, dot_vector, unit_vector};
use crate::topology::{Graph, Node};
#[cfg(test)]
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{
    analytic_surface_parameters, model_surface_partials_by_id, model_surface_point_by_id,
    nurbs_surface_closest_parameter, nurbs_surface_parameter_within_tolerance,
    nurbs_surface_partials, surface_partials,
};
use cadmpeg_ir::geometry::{
    knots_nondecreasing, IntcurveSupportSide, NurbsSurface, PcurveGeometry,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{BTreeMap, BTreeSet};

const OFFSET_NEWTON_ITERATIONS: usize = 32;
const OFFSET_PARAMETER_STEP_EPSILON: f64 = 1.0e-12;

pub(crate) fn saved_offset_carriers(
    ir: &CadIr,
    graph: &Graph,
    offsets: &[crate::topology::OffsetSurface],
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    tolerance: f64,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> BTreeMap<u32, (SurfaceId, f64)> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return BTreeMap::new();
    }
    let face_surfaces = graph
        .of_kind(14)
        .filter_map(Node::face_fields)
        .map(|face| face.surface)
        .collect::<BTreeSet<_>>();
    let candidates = face_surfaces
        .iter()
        .filter_map(|xmt| surfaces_by_xmt.get(xmt))
        .filter_map(|id| {
            let geometry = &ir
                .model
                .surfaces
                .iter()
                .find(|surface| &surface.id == id)?
                .geometry;
            matches!(geometry, SurfaceGeometry::Nurbs(_)).then_some((id, geometry))
        })
        .collect::<Vec<_>>();

    let mut matches = BTreeMap::<u32, Vec<(SurfaceId, f64)>>::new();
    let mut candidate_owners = BTreeMap::<SurfaceId, Vec<u32>>::new();
    for offset in offsets
        .iter()
        .filter(|offset| !face_surfaces.contains(&offset.xmt))
    {
        let Some(support_id) = surfaces_by_xmt.get(&offset.support) else {
            continue;
        };
        let Some(support) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| &surface.id == support_id)
            .map(|surface| &surface.geometry)
        else {
            continue;
        };
        for (candidate_id, candidate) in &candidates {
            if *candidate_id == support_id {
                continue;
            }
            if let Some(fit) = certified_offset_cache_fit_with_budget(
                support,
                candidate,
                offset.distance,
                tolerance,
                geometry_budget,
            ) {
                matches
                    .entry(offset.xmt)
                    .or_default()
                    .push(((*candidate_id).clone(), fit));
                candidate_owners
                    .entry((*candidate_id).clone())
                    .or_default()
                    .push(offset.xmt);
            }
        }
    }

    matches
        .into_iter()
        .filter_map(|(offset, candidates)| {
            let [(candidate, fit)] = candidates.as_slice() else {
                return None;
            };
            (candidate_owners.get(candidate).map(Vec::as_slice) == Some(&[offset][..]))
                .then(|| (offset, (candidate.clone(), *fit)))
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn certified_offset_cache_fit(
    support: &SurfaceGeometry,
    candidate: &SurfaceGeometry,
    distance: f64,
    tolerance: f64,
) -> Option<f64> {
    let geometry_budget = WorkBudget::new(MAX_ADAPTIVE_GEOMETRY_WORK);
    certified_offset_cache_fit_with_budget(
        support,
        candidate,
        distance,
        tolerance,
        &geometry_budget,
    )
}

pub(crate) fn certified_offset_cache_fit_with_budget(
    support: &SurfaceGeometry,
    candidate: &SurfaceGeometry,
    distance: f64,
    tolerance: f64,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Option<f64> {
    let (SurfaceGeometry::Nurbs(support), SurfaceGeometry::Nurbs(candidate)) = (support, candidate)
    else {
        return None;
    };
    let compatible_parameterization = support.u_degree > 0
        && support.v_degree > 0
        && candidate.u_degree > 0
        && candidate.v_degree > 0
        && support.u_periodic == candidate.u_periodic
        && support.v_periodic == candidate.v_periodic
        && nurbs_active_domain(support)
            .zip(nurbs_active_domain(candidate))
            .is_some_and(|(support, candidate)| support == candidate)
        && support
            .weights
            .as_ref()
            .is_none_or(|weights| weights.len() == support.control_points.len())
        && candidate
            .weights
            .as_ref()
            .is_none_or(|weights| weights.len() == candidate.control_points.len())
        && positive_weights(support.weights.as_deref())
        && positive_weights(candidate.weights.as_deref());
    if !compatible_parameterization
        || !distance.is_finite()
        || !tolerance.is_finite()
        || tolerance < 0.0
    {
        return None;
    }
    let same_basis = candidate.u_degree == support.u_degree
        && candidate.v_degree == support.v_degree
        && support.u_knots == candidate.u_knots
        && support.v_knots == candidate.v_knots
        && support.u_count == candidate.u_count
        && support.v_count == candidate.v_count
        && support.weights == candidate.weights
        && support.control_points.len() == candidate.control_points.len();
    if same_basis {
        if let Some(normal) = translation_net_normal(support) {
            let translation = Vector3::new(
                distance * normal.x,
                distance * normal.y,
                distance * normal.z,
            );
            let maximum_error = support
                .control_points
                .iter()
                .zip(&candidate.control_points)
                .map(|(support, candidate)| {
                    let expected = Point3::new(
                        support.x + translation.x,
                        support.y + translation.y,
                        support.z + translation.z,
                    );
                    point_distance(expected, *candidate)
                })
                .try_fold(0.0_f64, |maximum, error| {
                    error.is_finite().then(|| maximum.max(error))
                })?;
            return (maximum_error <= tolerance).then_some(maximum_error);
        }
    }
    certified_curved_offset_cache_fit_with_budget(
        support,
        candidate,
        distance,
        tolerance,
        same_basis,
        geometry_budget,
    )
}

pub(crate) fn nurbs_active_domain(surface: &NurbsSurface) -> Option<[[u64; 2]; 2]> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    Some([
        [
            surface.u_knots.get(u_degree)?.to_bits(),
            surface.u_knots.get(u_count)?.to_bits(),
        ],
        [
            surface.v_knots.get(v_degree)?.to_bits(),
            surface.v_knots.get(v_count)?.to_bits(),
        ],
    ])
}

#[derive(Clone)]
pub(crate) struct HomogeneousSurfaceNet {
    pub(crate) u_degree: usize,
    pub(crate) v_degree: usize,
    pub(crate) u_knots: Vec<f64>,
    pub(crate) v_knots: Vec<f64>,
    pub(crate) u_count: usize,
    pub(crate) v_count: usize,
    pub(crate) controls: Vec<[f64; 4]>,
}

impl HomogeneousSurfaceNet {
    fn from_homogeneous_surface(surface: &NurbsSurface) -> Option<Self> {
        Self::from_components(surface, |point, weight| {
            [point.x * weight, point.y * weight, point.z * weight, weight]
        })
    }

    fn from_homogeneous_residual(support: &NurbsSurface, candidate: &NurbsSurface) -> Option<Self> {
        Self::from_homogeneous_surface(candidate)?;
        let mut net = Self::from_components(support, |point, weight| {
            [point.x * weight, point.y * weight, point.z * weight, weight]
        })?;
        for ((control, support), candidate) in net
            .controls
            .iter_mut()
            .zip(&support.control_points)
            .zip(&candidate.control_points)
        {
            control[0] = (candidate.x - support.x) * control[3];
            control[1] = (candidate.y - support.y) * control[3];
            control[2] = (candidate.z - support.z) * control[3];
        }
        net.controls
            .iter()
            .flatten()
            .all(|component| component.is_finite())
            .then_some(net)
    }

    fn from_components(
        surface: &NurbsSurface,
        components: impl Fn(Point3, f64) -> [f64; 4],
    ) -> Option<Self> {
        let u_degree = usize::try_from(surface.u_degree).ok()?;
        let v_degree = usize::try_from(surface.v_degree).ok()?;
        let u_count = usize::try_from(surface.u_count).ok()?;
        let v_count = usize::try_from(surface.v_count).ok()?;
        let control_count = u_count.checked_mul(v_count)?;
        if u_degree == 0
            || v_degree == 0
            || u_degree >= u_count
            || v_degree >= v_count
            || surface.control_points.len() != control_count
            || surface.u_knots.len() != u_count.checked_add(u_degree)?.checked_add(1)?
            || surface.v_knots.len() != v_count.checked_add(v_degree)?.checked_add(1)?
            || surface
                .u_knots
                .iter()
                .chain(&surface.v_knots)
                .any(|knot| !knot.is_finite())
            || !knots_nondecreasing(&surface.u_knots)
            || !knots_nondecreasing(&surface.v_knots)
            || surface
                .control_points
                .iter()
                .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
            || !positive_weights(surface.weights.as_deref())
        {
            return None;
        }
        let controls = surface
            .control_points
            .iter()
            .enumerate()
            .map(|(index, point)| {
                components(
                    *point,
                    surface
                        .weights
                        .as_ref()
                        .map_or(1.0, |weights| weights[index]),
                )
            })
            .collect::<Vec<_>>();
        if controls
            .iter()
            .flatten()
            .any(|component| !component.is_finite())
        {
            return None;
        }
        Some(Self {
            u_degree,
            v_degree,
            u_knots: surface.u_knots.clone(),
            v_knots: surface.v_knots.clone(),
            u_count,
            v_count,
            controls,
        })
    }

    fn derivative(&self, u_axis: bool) -> Option<Self> {
        let (degree, count, knots) = if u_axis {
            (self.u_degree, self.u_count, &self.u_knots)
        } else {
            (self.v_degree, self.v_count, &self.v_knots)
        };
        if degree == 0 || count < 2 {
            return None;
        }
        let next_u_count = self.u_count - usize::from(u_axis);
        let next_v_count = self.v_count - usize::from(!u_axis);
        let mut controls = Vec::with_capacity(next_u_count.checked_mul(next_v_count)?);
        for u in 0..next_u_count {
            for v in 0..next_v_count {
                let index = |u, v| u * self.v_count + v;
                let (first, second, derivative_index) = if u_axis {
                    (
                        self.controls[index(u, v)],
                        self.controls[index(u + 1, v)],
                        u,
                    )
                } else {
                    (
                        self.controls[index(u, v)],
                        self.controls[index(u, v + 1)],
                        v,
                    )
                };
                let denominator =
                    knots[derivative_index + degree + 1] - knots[derivative_index + 1];
                if !denominator.is_finite() || denominator < 0.0 {
                    return None;
                }
                if denominator == 0.0 {
                    controls.push([0.0; 4]);
                    continue;
                }
                let factor = degree as f64 / denominator;
                controls.push(std::array::from_fn(|axis| {
                    factor * (second[axis] - first[axis])
                }));
            }
        }
        Some(Self {
            u_degree: self.u_degree - usize::from(u_axis),
            v_degree: self.v_degree - usize::from(!u_axis),
            u_knots: if u_axis {
                self.u_knots[1..self.u_knots.len() - 1].to_vec()
            } else {
                self.u_knots.clone()
            },
            v_knots: if u_axis {
                self.v_knots.clone()
            } else {
                self.v_knots[1..self.v_knots.len() - 1].to_vec()
            },
            u_count: next_u_count,
            v_count: next_v_count,
            controls,
        })
    }

    fn active_control_bounds(
        &self,
        u: f64,
        v: f64,
        origin: [f64; 3],
    ) -> Option<HomogeneousControlBounds> {
        let u_controls = active_spline_controls(&self.u_knots, self.u_degree, self.u_count, u)?;
        let v_controls = active_spline_controls(&self.v_knots, self.v_degree, self.v_count, v)?;
        let mut bounds = HomogeneousControlBounds {
            minimum_weight: f64::INFINITY,
            maximum_position_norm: 0.0,
            maximum_weight_magnitude: 0.0,
        };
        for u in u_controls {
            for v in v_controls.clone() {
                let control = self.controls[u * self.v_count + v];
                let position_norm = control[..3]
                    .iter()
                    .zip(origin)
                    .map(|(coordinate, origin)| {
                        let coordinate = coordinate - origin * control[3];
                        coordinate * coordinate
                    })
                    .sum::<f64>()
                    .sqrt();
                if !position_norm.is_finite() || !control[3].is_finite() {
                    return None;
                }
                bounds.minimum_weight = bounds.minimum_weight.min(control[3]);
                bounds.maximum_position_norm = bounds.maximum_position_norm.max(position_norm);
                bounds.maximum_weight_magnitude =
                    bounds.maximum_weight_magnitude.max(control[3].abs());
            }
        }
        Some(bounds)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct HomogeneousControlBounds {
    pub(crate) minimum_weight: f64,
    pub(crate) maximum_position_norm: f64,
    pub(crate) maximum_weight_magnitude: f64,
}

pub(crate) fn active_spline_controls(
    knots: &[f64],
    degree: usize,
    count: usize,
    parameter: f64,
) -> Option<std::ops::RangeInclusive<usize>> {
    let lower = *knots.get(degree)?;
    let upper = *knots.get(count)?;
    if !parameter.is_finite() || parameter < lower || parameter > upper || lower >= upper {
        return None;
    }
    let span = if parameter == upper {
        count.checked_sub(1)?
    } else {
        knots
            .partition_point(|knot| *knot <= parameter)
            .checked_sub(1)?
    };
    (span >= degree && span < count).then_some(span - degree..=span)
}

pub(crate) fn certified_curved_offset_cache_fit_with_budget(
    support: &NurbsSurface,
    candidate: &NurbsSurface,
    distance: f64,
    tolerance: f64,
    same_basis: bool,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Option<f64> {
    let support_net = HomogeneousSurfaceNet::from_homogeneous_surface(support)?;
    let candidate_net = HomogeneousSurfaceNet::from_homogeneous_surface(candidate)?;
    let residual_net = if same_basis {
        Some(HomogeneousSurfaceNet::from_homogeneous_residual(
            support, candidate,
        )?)
    } else {
        None
    };
    let support_derivatives = RationalSurfaceDerivativeNets::from_net(&support_net)?;
    let candidate_derivatives = RationalSurfaceDerivativeNets::from_net(&candidate_net)?;
    let residual_derivatives = match residual_net.as_ref() {
        Some(net) => Some(RationalSurfaceDerivativeNets::from_net(net)?),
        None => None,
    };

    let mut u_breaks = support_net.u_knots[support_net.u_degree..=support_net.u_count].to_vec();
    u_breaks.extend(&candidate_net.u_knots[candidate_net.u_degree..=candidate_net.u_count]);
    u_breaks.sort_by(f64::total_cmp);
    u_breaks.dedup();
    let mut v_breaks = support_net.v_knots[support_net.v_degree..=support_net.v_count].to_vec();
    v_breaks.extend(&candidate_net.v_knots[candidate_net.v_degree..=candidate_net.v_count]);
    v_breaks.sort_by(f64::total_cmp);
    v_breaks.dedup();
    let mut rectangles = u_breaks
        .windows(2)
        .filter(|span| span[0] < span[1])
        .flat_map(|u| {
            v_breaks
                .windows(2)
                .filter(|span| span[0] < span[1])
                .map(move |v| [u[0], u[1], v[0], v[1]])
        })
        .collect::<Vec<_>>();
    if rectangles.is_empty() {
        return None;
    }
    let mut certified_bound = 0.0_f64;
    while let Some([u0, u1, v0, v1]) = rectangles.pop() {
        if !geometry_budget.charge() {
            return None;
        }
        let u = u0 + (u1 - u0) * 0.5;
        let v = v0 + (v1 - v0) * 0.5;
        let support_bounds =
            rational_surface_derivative_bounds_with_nets(&support_net, &support_derivatives, u, v)?;
        let (residual_u_bound, residual_v_bound) = if let (Some(residual_net), Some(derivatives)) =
            (&residual_net, &residual_derivatives)
        {
            let bounds =
                rational_surface_derivative_bounds_with_nets(residual_net, derivatives, u, v)?;
            (bounds.u, bounds.v)
        } else {
            let candidate_bounds = rational_surface_derivative_bounds_with_nets(
                &candidate_net,
                &candidate_derivatives,
                u,
                v,
            )?;
            (
                support_bounds.u + candidate_bounds.u,
                support_bounds.v + candidate_bounds.v,
            )
        };
        let normal_u_numerator =
            support_bounds.uu * support_bounds.v + support_bounds.u * support_bounds.uv;
        let normal_v_numerator =
            support_bounds.uv * support_bounds.v + support_bounds.u * support_bounds.vv;
        if !normal_u_numerator.is_finite() || !normal_v_numerator.is_finite() {
            return None;
        }
        let support_point = cadmpeg_ir::eval::nurbs_surface_point(support, u, v)?;
        let candidate_point = cadmpeg_ir::eval::nurbs_surface_point(candidate, u, v)?;
        let partials = nurbs_surface_partials(support, u, v)?;
        let normal_vector = cross_vector(partials.du, partials.dv);
        let normal_size = normal_vector.norm();
        let half_u = (u1 - u0) * 0.5;
        let half_v = (v1 - v0) * 0.5;
        let minimum_normal =
            normal_size - normal_u_numerator * half_u - normal_v_numerator * half_v;
        if !minimum_normal.is_finite() || minimum_normal <= 0.0 {
            let split_u = normal_u_numerator * (u1 - u0) >= normal_v_numerator * (v1 - v0);
            if !subdivide_offset_rectangle(&mut rectangles, [u0, u1, v0, v1], [u, v], split_u) {
                return None;
            }
            continue;
        }
        let normal = unit_vector(normal_vector)?;
        let u_lipschitz = residual_u_bound + distance.abs() * normal_u_numerator / minimum_normal;
        let v_lipschitz = residual_v_bound + distance.abs() * normal_v_numerator / minimum_normal;
        let expected = Point3::new(
            support_point.x + distance * normal.x,
            support_point.y + distance * normal.y,
            support_point.z + distance * normal.z,
        );
        let midpoint_error = point_distance(expected, candidate_point);
        let bound = midpoint_error + u_lipschitz * half_u + v_lipschitz * half_v;
        if !bound.is_finite() {
            return None;
        }
        if bound <= tolerance {
            certified_bound = certified_bound.max(bound);
            continue;
        }
        let split_u = u_lipschitz * (u1 - u0) >= v_lipschitz * (v1 - v0);
        if !subdivide_offset_rectangle(&mut rectangles, [u0, u1, v0, v1], [u, v], split_u) {
            return None;
        }
    }
    Some(certified_bound)
}

#[derive(Clone, Copy)]
pub(crate) struct RationalSurfaceDerivativeBounds {
    pub(crate) u: f64,
    pub(crate) v: f64,
    pub(crate) uu: f64,
    pub(crate) uv: f64,
    pub(crate) vv: f64,
}

struct RationalSurfaceDerivativeNets {
    u: HomogeneousSurfaceNet,
    v: HomogeneousSurfaceNet,
    uv: HomogeneousSurfaceNet,
    uu: Option<HomogeneousSurfaceNet>,
    vv: Option<HomogeneousSurfaceNet>,
}

impl RationalSurfaceDerivativeNets {
    fn from_net(net: &HomogeneousSurfaceNet) -> Option<Self> {
        let u = net.derivative(true)?;
        let v = net.derivative(false)?;
        let uv = u.derivative(false)?;
        let uu = if u.u_degree == 0 {
            None
        } else {
            Some(u.derivative(true)?)
        };
        let vv = if v.v_degree == 0 {
            None
        } else {
            Some(v.derivative(false)?)
        };
        Some(Self { u, v, uv, uu, vv })
    }
}

fn rational_surface_derivative_bounds_with_nets(
    net: &HomogeneousSurfaceNet,
    derivatives: &RationalSurfaceDerivativeNets,
    u: f64,
    v: f64,
) -> Option<RationalSurfaceDerivativeBounds> {
    let u_controls = active_spline_controls(&net.u_knots, net.u_degree, net.u_count, u)?;
    let v_controls = active_spline_controls(&net.v_knots, net.v_degree, net.v_count, v)?;
    let reference = net.controls[*u_controls.start() * net.v_count + *v_controls.start()];
    let origin = (reference[3] > 0.0).then(|| {
        [
            reference[0] / reference[3],
            reference[1] / reference[3],
            reference[2] / reference[3],
        ]
    })?;
    if origin.iter().any(|coordinate| !coordinate.is_finite()) {
        return None;
    }
    let base_bounds = net.active_control_bounds(u, v, origin)?;
    let weight_floor = (base_bounds.minimum_weight > 0.0).then_some(base_bounds.minimum_weight)?;
    let a = base_bounds.maximum_position_norm;
    let u_bounds = derivatives.u.active_control_bounds(u, v, origin)?;
    let v_bounds = derivatives.v.active_control_bounds(u, v, origin)?;
    let au = u_bounds.maximum_position_norm;
    let av = v_bounds.maximum_position_norm;
    let wu = u_bounds.maximum_weight_magnitude;
    let wv = v_bounds.maximum_weight_magnitude;
    let (auu, wuu) = derivatives.uu.as_ref().map_or(Some((0.0, 0.0)), |net| {
        let bounds = net.active_control_bounds(u, v, origin)?;
        Some((
            bounds.maximum_position_norm,
            bounds.maximum_weight_magnitude,
        ))
    })?;
    let (avv, wvv) = derivatives.vv.as_ref().map_or(Some((0.0, 0.0)), |net| {
        let bounds = net.active_control_bounds(u, v, origin)?;
        Some((
            bounds.maximum_position_norm,
            bounds.maximum_weight_magnitude,
        ))
    })?;
    let uv_bounds = derivatives.uv.active_control_bounds(u, v, origin)?;
    let auv = uv_bounds.maximum_position_norm;
    let wuv = uv_bounds.maximum_weight_magnitude;
    let inverse_weight = weight_floor.recip();
    let inverse_weight_squared = inverse_weight * inverse_weight;
    let inverse_weight_cubed = inverse_weight_squared * inverse_weight;
    let u = au * inverse_weight + a * wu * inverse_weight_squared;
    let v = av * inverse_weight + a * wv * inverse_weight_squared;
    let uu = auu * inverse_weight
        + (a * wuu + 2.0 * au * wu) * inverse_weight_squared
        + 2.0 * a * wu * wu * inverse_weight_cubed;
    let uv = auv * inverse_weight
        + (au * wv + av * wu + a * wuv) * inverse_weight_squared
        + 2.0 * a * wu * wv * inverse_weight_cubed;
    let vv = avv * inverse_weight
        + (a * wvv + 2.0 * av * wv) * inverse_weight_squared
        + 2.0 * a * wv * wv * inverse_weight_cubed;
    [u, v, uu, uv, vv]
        .iter()
        .all(|bound| bound.is_finite())
        .then_some(RationalSurfaceDerivativeBounds { u, v, uu, uv, vv })
}

pub(crate) fn subdivide_offset_rectangle(
    rectangles: &mut Vec<[f64; 4]>,
    [u0, u1, v0, v1]: [f64; 4],
    [u, v]: [f64; 2],
    split_u: bool,
) -> bool {
    let u_divisible = u != u0 && u != u1;
    let v_divisible = v != v0 && v != v1;
    if u_divisible && (split_u || !v_divisible) {
        rectangles.extend([[u0, u, v0, v1], [u, u1, v0, v1]]);
        true
    } else if v_divisible {
        rectangles.extend([[u0, u1, v0, v], [u0, u1, v, v1]]);
        true
    } else {
        false
    }
}

pub(crate) fn translation_net_normal(surface: &NurbsSurface) -> Option<Vector3> {
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    if u_count < 2
        || v_count < 2
        || u_degree >= u_count
        || v_degree >= v_count
        || surface.control_points.len() != u_count.checked_mul(v_count)?
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
        || surface.u_knots.len() != u_count.checked_add(u_degree)?.checked_add(1)?
        || surface.v_knots.len() != v_count.checked_add(v_degree)?.checked_add(1)?
    {
        return None;
    }
    let point = |u: usize, v: usize| surface.control_points[u * v_count + v];
    let difference = |end: Point3, start: Point3| {
        Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z)
    };
    let u_direction = difference(point(1, 0), point(0, 0));
    let v_direction = difference(point(0, 1), point(0, 0));
    let normal = unit_vector(cross_vector(u_direction, v_direction))?;

    let positive_collinear = |increment: Vector3, direction: Vector3| {
        increment.x.is_finite()
            && increment.y.is_finite()
            && increment.z.is_finite()
            && cross_vector(increment, direction) == Vector3::new(0.0, 0.0, 0.0)
            && dot_vector(increment, direction) > 0.0
    };
    for u in 0..u_count - 1 {
        let denominator = surface.u_knots[u + u_degree + 1] - surface.u_knots[u + 1];
        if !denominator.is_finite()
            || denominator <= 0.0
            || !positive_collinear(difference(point(u + 1, 0), point(u, 0)), u_direction)
        {
            return None;
        }
    }
    for v in 0..v_count - 1 {
        let denominator = surface.v_knots[v + v_degree + 1] - surface.v_knots[v + 1];
        if !denominator.is_finite()
            || denominator <= 0.0
            || !positive_collinear(difference(point(0, v + 1), point(0, v)), v_direction)
        {
            return None;
        }
    }
    let origin = point(0, 0);
    for u in 0..u_count {
        for v in 0..v_count {
            let v_displacement = difference(point(0, v), origin);
            if difference(point(u, v), point(u, 0)) != v_displacement {
                return None;
            }
        }
    }
    Some(normal)
}

pub(crate) fn positive_weights(weights: Option<&[f64]>) -> bool {
    let Some(weights) = weights else {
        return true;
    };
    !weights.is_empty()
        && weights
            .iter()
            .all(|weight| weight.is_finite() && *weight > 0.0)
}

#[cfg(test)]
pub(crate) fn offset_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    offset_surface_parameters_with_tolerance(ir, surface, point, seed, None)
}

#[cfg(test)]
pub(crate) fn offset_surface_parameters_with_tolerance(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
) -> Option<Point2> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    offset_surface_parameters_with_tolerance_with_index(&index, surface, point, seed, fit_tolerance)
}

#[cfg(test)]
pub(crate) fn offset_surface_parameters_with_tolerance_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
) -> Option<Point2> {
    let geometry_budget = WorkBudget::new(MAX_ADAPTIVE_GEOMETRY_WORK);
    offset_surface_parameters_with_tolerance_with_index_and_budget(
        index,
        surface,
        point,
        seed,
        fit_tolerance,
        &geometry_budget,
    )
}

pub(crate) fn offset_surface_parameters_with_tolerance_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Option<Point2> {
    (!geometry_budget.exhausted()).then_some(())?;
    let ir = index.ir();
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let SurfaceGeometry::Procedural { construction } = &carrier.geometry else {
        return None;
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|candidate| &candidate.id == construction && &candidate.surface == surface)?;
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = &procedural.definition
    else {
        return None;
    };
    let domain = surface_parameter_domain(ir, support);
    // The target lies on the offset carrier, so its distance from the base
    // carrier may be the full offset distance even for an exact fit. Enlarge
    // only the base-surface seed search; the iterations and final caller-side
    // evaluation still certify the requested tolerance on the offset itself.
    let support_fit_tolerance = fit_tolerance.and_then(|tolerance| {
        let tolerance = tolerance + distance.abs();
        tolerance.is_finite().then_some(tolerance)
    });
    if fit_tolerance.is_some_and(|tolerance| !tolerance.is_finite() || tolerance < 0.0) {
        return None;
    }
    let mut starts = Vec::with_capacity(3);
    let mut add_start = |candidate: Option<Point2>| {
        let Some(mut candidate) = candidate else {
            return;
        };
        if !candidate.u.is_finite() || !candidate.v.is_finite() {
            return;
        }
        clamp_surface_parameters(&mut candidate, domain);
        if !starts.contains(&candidate) {
            starts.push(candidate);
        }
    };
    add_start(seed);
    add_start(initial_surface_parameters(
        ir,
        support,
        point,
        None,
        support_fit_tolerance,
    ));
    add_start(
        domain.and_then(|domain| coarse_model_surface_parameters(index, surface, point, domain)),
    );

    let mut best = None;
    for mut parameters in starts {
        for _ in 0..OFFSET_NEWTON_ITERATIONS {
            if !geometry_budget.charge() {
                break;
            }
            let Some(position) =
                model_surface_point_by_id(index, surface, parameters.u, parameters.v)
            else {
                break;
            };
            let residual = Vector3::new(
                position.x - point.x,
                position.y - point.y,
                position.z - point.z,
            );
            if fit_tolerance
                .is_some_and(|tolerance| dot_vector(residual, residual) <= tolerance * tolerance)
            {
                break;
            }
            let u_step = parameter_derivative_step(parameters.u, domain.map(|domain| domain.0));
            let v_step = parameter_derivative_step(parameters.v, domain.map(|domain| domain.1));
            let Some(du) = model_surface_derivative(
                index,
                surface,
                parameters,
                u_step,
                true,
                domain,
                [None, None],
            ) else {
                break;
            };
            let Some(dv) = model_surface_derivative(
                index,
                surface,
                parameters,
                v_step,
                false,
                domain,
                [None, None],
            ) else {
                break;
            };
            let Some((step_u, step_v)) = least_squares_step(du, dv, residual) else {
                break;
            };
            parameters.u -= step_u;
            parameters.v -= step_v;
            clamp_surface_parameters(&mut parameters, domain);
            if step_u.abs() <= OFFSET_PARAMETER_STEP_EPSILON * (1.0 + parameters.u.abs())
                && step_v.abs() <= OFFSET_PARAMETER_STEP_EPSILON * (1.0 + parameters.v.abs())
            {
                break;
            }
        }
        let Some(position) = model_surface_point_by_id(index, surface, parameters.u, parameters.v)
        else {
            continue;
        };
        let residual = point_distance(position, point);
        if !residual.is_finite() {
            continue;
        }
        if fit_tolerance.is_some_and(|tolerance| residual <= tolerance) {
            return Some(parameters);
        }
        let replace = best.is_none_or(|(_, best_residual)| residual < best_residual);
        if replace {
            best = Some((parameters, residual));
        }
    }
    fit_tolerance
        .is_none()
        .then(|| best.map(|(parameters, _)| parameters))?
}

pub(crate) fn coarse_model_surface_parameters(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    domain: ([f64; 2], [f64; 2]),
) -> Option<Point2> {
    let (u_domain, v_domain) = domain;
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for ui in 0..=8 {
        for vi in 0..=8 {
            let parameters = Point2::new(
                u_domain[0] + (u_domain[1] - u_domain[0]) * f64::from(ui) / 8.0,
                v_domain[0] + (v_domain[1] - v_domain[0]) * f64::from(vi) / 8.0,
            );
            let Some(candidate) =
                model_surface_point_by_id(index, surface, parameters.u, parameters.v)
            else {
                continue;
            };
            let distance = (candidate.x - point.x).powi(2)
                + (candidate.y - point.y).powi(2)
                + (candidate.z - point.z).powi(2);
            if distance < best_distance {
                best = Some(parameters);
                best_distance = distance;
            }
        }
    }
    best
}

pub(crate) fn initial_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
) -> Option<Point2> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => fit_tolerance.map_or_else(
            || nurbs_surface_closest_parameter(nurbs, point, seed),
            |tolerance| nurbs_surface_parameter_within_tolerance(nurbs, point, seed, tolerance),
        ),
        SurfaceGeometry::Procedural { construction } => {
            let procedural =
                ir.model.procedural_surfaces.iter().find(|candidate| {
                    &candidate.id == construction && &candidate.surface == surface
                })?;
            let ProceduralSurfaceDefinition::Offset {
                support, distance, ..
            } = &procedural.definition
            else {
                return None;
            };
            let support_fit_tolerance = fit_tolerance.and_then(|tolerance| {
                let tolerance = tolerance + distance.abs();
                tolerance.is_finite().then_some(tolerance)
            });
            initial_surface_parameters(ir, support, point, seed, support_fit_tolerance)
        }
        geometry => analytic_surface_parameters(geometry, point),
    }
}

pub(crate) fn surface_parameter_domain(
    ir: &CadIr,
    surface: &SurfaceId,
) -> Option<([f64; 2], [f64; 2])> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => {
            let u_degree = usize::try_from(nurbs.u_degree).ok()?;
            let v_degree = usize::try_from(nurbs.v_degree).ok()?;
            let u_count = usize::try_from(nurbs.u_count).ok()?;
            let v_count = usize::try_from(nurbs.v_count).ok()?;
            Some((
                [*nurbs.u_knots.get(u_degree)?, *nurbs.u_knots.get(u_count)?],
                [*nurbs.v_knots.get(v_degree)?, *nurbs.v_knots.get(v_count)?],
            ))
        }
        SurfaceGeometry::Procedural { construction } => {
            let procedural =
                ir.model.procedural_surfaces.iter().find(|candidate| {
                    &candidate.id == construction && &candidate.surface == surface
                })?;
            let ProceduralSurfaceDefinition::Offset { support, .. } = &procedural.definition else {
                return None;
            };
            surface_parameter_domain(ir, support)
        }
        _ => None,
    }
}

pub(crate) fn clamp_surface_parameters(
    parameters: &mut Point2,
    domain: Option<([f64; 2], [f64; 2])>,
) {
    if let Some((u_domain, v_domain)) = domain {
        parameters.u = parameters.u.clamp(u_domain[0], u_domain[1]);
        parameters.v = parameters.v.clamp(v_domain[0], v_domain[1]);
    }
}

pub(crate) fn parameter_derivative_step(parameter: f64, domain: Option<[f64; 2]>) -> f64 {
    domain.map_or_else(
        || 1.0e-6 * (1.0 + parameter.abs()),
        |domain| 1.0e-6 * (domain[1] - domain[0]).abs().max(1.0),
    )
}

pub(crate) fn model_surface_derivative(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    parameters: Point2,
    step: f64,
    along_u: bool,
    domain: Option<([f64; 2], [f64; 2])>,
    periods: [Option<f64>; 2],
) -> Option<Vector3> {
    let carrier = index.surfaces(&surface.0)?;
    if let Some(partials) = surface_partials(&carrier.geometry, parameters.u, parameters.v) {
        return Some(if along_u { partials.du } else { partials.dv });
    }
    if let Some(partials) = model_surface_partials_by_id(index, surface, parameters.u, parameters.v)
    {
        return Some(if along_u { partials.du } else { partials.dv });
    }

    let mut before = parameters;
    let mut after = parameters;
    if along_u {
        before.u -= step;
        after.u += step;
    } else {
        before.v -= step;
        after.v += step;
    }
    clamp_surface_parameters_with_periods(&mut before, domain, periods);
    clamp_surface_parameters_with_periods(&mut after, domain, periods);
    let width = if along_u {
        after.u - before.u
    } else {
        after.v - before.v
    };
    if !width.is_finite() || width == 0.0 {
        return None;
    }
    let first = model_surface_point_by_id(index, surface, before.u, before.v)?;
    let second = model_surface_point_by_id(index, surface, after.u, after.v)?;
    Some(Vector3::new(
        (second.x - first.x) / width,
        (second.y - first.y) / width,
        (second.z - first.z) / width,
    ))
}

/// Continue one chart-selected surface-intersection branch in both support
/// parameter spaces. The chart seeds and orders the branch; corrected points
/// satisfy the two support surfaces rather than interpolating chart samples.
#[cfg(test)]
pub(crate) fn continue_surface_intersection_parameters(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    chart: &[Point3],
    fit_tolerance: f64,
) -> Option<[Vec<Point2>; 2]> {
    continue_surface_intersection_parameters_with_seeds(
        ir,
        surfaces,
        chart,
        fit_tolerance,
        [None, None],
    )
}

#[cfg(test)]
pub(crate) fn continue_surface_intersection_parameters_with_seeds(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    chart: &[Point3],
    fit_tolerance: f64,
    seeds: [Option<Point2>; 2],
) -> Option<[Vec<Point2>; 2]> {
    let geometry_budget = WorkBudget::new(MAX_ADAPTIVE_GEOMETRY_WORK);
    continue_surface_intersection_parameters_with_seeds_and_budget(
        ir,
        surfaces,
        chart,
        fit_tolerance,
        seeds,
        &geometry_budget,
    )
}

pub(crate) fn continue_surface_intersection_parameters_with_seeds_and_budget(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    chart: &[Point3],
    fit_tolerance: f64,
    seeds: [Option<Point2>; 2],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Option<[Vec<Point2>; 2]> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    if chart.len() < 2
        || surfaces[0] == surfaces[1]
        || !fit_tolerance.is_finite()
        || fit_tolerance <= 0.0
    {
        return None;
    }
    let fit_parameters = |surface: &SurfaceId, point: Point3, seed: Option<Point2>| {
        let geometry = &ir
            .model
            .surfaces
            .iter()
            .find(|candidate| &candidate.id == surface)?
            .geometry;
        match geometry {
            SurfaceGeometry::Nurbs(nurbs) => {
                nurbs_surface_parameter_within_tolerance(nurbs, point, seed, fit_tolerance)
            }
            SurfaceGeometry::Procedural { .. } => {
                offset_surface_parameters_with_tolerance_with_index_and_budget(
                    &index,
                    surface,
                    point,
                    seed,
                    Some(fit_tolerance),
                    geometry_budget,
                )
                .or_else(|| {
                    blend_surface_parameters_for_fit_with_grid_and_budget(
                        &index,
                        surface,
                        point,
                        seed,
                        fit_tolerance,
                        BlendParameterGrid::Build,
                        geometry_budget,
                    )
                })
            }
            geometry => analytic_surface_parameters(geometry, point),
        }
    };
    let first = [
        fit_parameters(surfaces[0], chart[0], seeds[0])?,
        fit_parameters(surfaces[1], chart[0], seeds[1])?,
    ];
    let space = IntersectionParameterSpace {
        domains: surfaces.map(|surface| surface_parameter_domain(ir, surface)),
        periods: surfaces.map(|surface| surface_parameter_periods(ir, surface)),
    };
    let seed = [first[0].u, first[0].v, first[1].u, first[1].v];
    let first_chord = Vector3::new(
        chart[1].x - chart[0].x,
        chart[1].y - chart[0].y,
        chart[1].z - chart[0].z,
    );
    let seed_tangent = intersection_parameter_tangent(&index, surfaces, seed, space, first_chord)?;
    let mut current = correct_intersection_parameters(
        &index,
        surfaces,
        seed,
        seed_tangent,
        space,
        fit_tolerance,
        1.0,
    )?;
    let first_point = model_surface_point_by_id(&index, surfaces[0], current[0], current[1])?;
    if point_distance(first_point, chart[0]) > fit_tolerance {
        return None;
    }
    let mut lanes = [
        vec![Point2::new(current[0], current[1])],
        vec![Point2::new(current[2], current[3])],
    ];

    for chart_pair in chart.windows(2) {
        let jacobian = intersection_parameter_jacobian(&index, surfaces, current, space)?;
        let chord = Vector3::new(
            chart_pair[1].x - chart_pair[0].x,
            chart_pair[1].y - chart_pair[0].y,
            chart_pair[1].z - chart_pair[0].z,
        );
        let tangent = intersection_parameter_tangent(&index, surfaces, current, space, chord)?;
        let spatial_tangent = Vector3::new(
            jacobian[0][0] * tangent[0] + jacobian[0][1] * tangent[1],
            jacobian[1][0] * tangent[0] + jacobian[1][1] * tangent[1],
            jacobian[2][0] * tangent[0] + jacobian[2][1] * tangent[1],
        );
        let target = [
            fit_parameters(
                surfaces[0],
                chart_pair[1],
                Some(Point2::new(current[0], current[1])),
            )?,
            fit_parameters(
                surfaces[1],
                chart_pair[1],
                Some(Point2::new(current[2], current[3])),
            )?,
        ];
        let mut predictor = [target[0].u, target[0].v, target[1].u, target[1].v];
        for (side, surface_periods) in space.periods.into_iter().enumerate() {
            for (coordinate, period) in surface_periods.into_iter().enumerate() {
                let index = side * 2 + coordinate;
                if let Some(period) = period {
                    predictor[index] =
                        lift_periodic_parameter(predictor[index], current[index], period);
                }
            }
        }
        let scale = (0..4)
            .map(|index| (predictor[index] - current[index]) * tangent[index])
            .sum::<f64>();
        if !scale.is_finite() || scale == 0.0 || dot_vector(spatial_tangent, chord) * scale <= 0.0 {
            return None;
        }
        let corrected = correct_intersection_parameters(
            &index,
            surfaces,
            predictor,
            tangent,
            space,
            fit_tolerance,
            scale,
        )?;
        let point = model_surface_point_by_id(&index, surfaces[0], corrected[0], corrected[1])?;
        if point_distance(point, chart_pair[1]) > fit_tolerance {
            return None;
        }
        current = corrected;
        lanes[0].push(Point2::new(current[0], current[1]));
        lanes[1].push(Point2::new(current[2], current[3]));
    }
    Some(lanes)
}

pub(crate) fn lift_periodic_parameter(value: f64, reference: f64, period: f64) -> f64 {
    value + ((reference - value) / period).round() * period
}

/// Return supported parameter periods while rejecting cyclic procedural support graphs.
pub(crate) fn surface_parameter_periods(ir: &CadIr, surface: &SurfaceId) -> [Option<f64>; 2] {
    surface_parameter_periods_inner(ir, surface, &mut BTreeSet::new())
}

pub(crate) fn surface_parameter_periods_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    visiting: &mut BTreeSet<SurfaceId>,
) -> [Option<f64>; 2] {
    if !visiting.insert(surface.clone()) {
        return [None, None];
    }
    let Some(carrier) = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)
    else {
        visiting.remove(surface);
        return [None, None];
    };
    let periods = match &carrier.geometry {
        SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. } => [Some(std::f64::consts::TAU), None],
        SurfaceGeometry::Torus { .. } => [Some(std::f64::consts::TAU), Some(std::f64::consts::TAU)],
        SurfaceGeometry::Nurbs(nurbs) => {
            let period = |periodic: bool, knots: &[f64], degree: u32, count: u32| {
                periodic.then(|| {
                    let degree = usize::try_from(degree).ok()?;
                    let count = usize::try_from(count).ok()?;
                    let period = knots.get(count)? - knots.get(degree)?;
                    (period.is_finite() && period > 0.0).then_some(period)
                })?
            };
            [
                period(
                    nurbs.u_periodic,
                    &nurbs.u_knots,
                    nurbs.u_degree,
                    nurbs.u_count,
                ),
                period(
                    nurbs.v_periodic,
                    &nurbs.v_knots,
                    nurbs.v_degree,
                    nurbs.v_count,
                ),
            ]
        }
        SurfaceGeometry::Procedural { construction } => ir
            .model
            .procedural_surfaces
            .iter()
            .find(|candidate| &candidate.id == construction && &candidate.surface == surface)
            .and_then(|procedural| match &procedural.definition {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    Some(surface_parameter_periods_inner(ir, support, visiting))
                }
                _ => None,
            })
            .unwrap_or([None, None]),
        _ => [None, None],
    };
    visiting.remove(surface);
    periods
}

pub(crate) fn correct_intersection_parameters(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces: [&SurfaceId; 2],
    predictor: [f64; 4],
    tangent: [f64; 4],
    space: IntersectionParameterSpace,
    fit_tolerance: f64,
    scale: f64,
) -> Option<[f64; 4]> {
    let mut corrected = predictor;
    clamp_intersection_parameters(&mut corrected, space);
    for _ in 0..32 {
        let first = model_surface_point_by_id(index, surfaces[0], corrected[0], corrected[1])?;
        let second = model_surface_point_by_id(index, surfaces[1], corrected[2], corrected[3])?;
        let residual = [
            first.x - second.x,
            first.y - second.y,
            first.z - second.z,
            (0..4)
                .map(|index| (corrected[index] - predictor[index]) * tangent[index])
                .sum(),
        ];
        let equality_error = residual[..3]
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if equality_error <= fit_tolerance * 1.0e-6
            && residual[3].abs() <= 1.0e-11 * (1.0 + scale.abs())
        {
            return Some(corrected);
        }
        let jacobian = intersection_parameter_jacobian(index, surfaces, corrected, space)?;
        let matrix = [jacobian[0], jacobian[1], jacobian[2], tangent];
        let rhs = residual.map(|value| -value);
        let step =
            solve_4x4(matrix, rhs).or_else(|| solve_damped_least_squares_4x4(matrix, rhs))?;
        for index in 0..4 {
            corrected[index] += step[index];
        }
        clamp_intersection_parameters(&mut corrected, space);
    }
    None
}

#[derive(Clone, Copy)]
pub(crate) struct IntersectionParameterSpace {
    pub(crate) domains: [Option<([f64; 2], [f64; 2])>; 2],
    pub(crate) periods: [[Option<f64>; 2]; 2],
}

pub(crate) fn intersection_parameter_tangent(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces: [&SurfaceId; 2],
    parameters: [f64; 4],
    space: IntersectionParameterSpace,
    chord: Vector3,
) -> Option<[f64; 4]> {
    let jacobian = intersection_parameter_jacobian(index, surfaces, parameters, space)?;
    if let Some(tangent) = null_vector_3x4(jacobian) {
        return Some(tangent);
    }
    let chord = unit_vector(chord)?;
    let derivatives = [
        [
            Vector3::new(jacobian[0][0], jacobian[1][0], jacobian[2][0]),
            Vector3::new(jacobian[0][1], jacobian[1][1], jacobian[2][1]),
        ],
        [
            Vector3::new(-jacobian[0][2], -jacobian[1][2], -jacobian[2][2]),
            Vector3::new(-jacobian[0][3], -jacobian[1][3], -jacobian[2][3]),
        ],
    ];
    let mut tangent = [0.0; 4];
    for side in 0..2 {
        let (u, v) = least_squares_step(derivatives[side][0], derivatives[side][1], chord)?;
        let mapped = unit_vector(Vector3::new(
            derivatives[side][0].x * u + derivatives[side][1].x * v,
            derivatives[side][0].y * u + derivatives[side][1].y * v,
            derivatives[side][0].z * u + derivatives[side][1].z * v,
        ))?;
        if dot_vector(mapped, chord) < 1.0 - 1.0e-8 {
            return None;
        }
        tangent[side * 2] = u;
        tangent[side * 2 + 1] = v;
    }
    let norm = tangent
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    (norm.is_finite() && norm > 1.0e-14).then(|| tangent.map(|value| value / norm))
}

pub(crate) fn intersection_parameter_jacobian(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces: [&SurfaceId; 2],
    parameters: [f64; 4],
    space: IntersectionParameterSpace,
) -> Option<[[f64; 4]; 3]> {
    let pairs = [
        Point2::new(parameters[0], parameters[1]),
        Point2::new(parameters[2], parameters[3]),
    ];
    let derivatives = std::array::from_fn(|side| {
        let u_step =
            parameter_derivative_step(pairs[side].u, space.domains[side].map(|value| value.0));
        let v_step =
            parameter_derivative_step(pairs[side].v, space.domains[side].map(|value| value.1));
        Some([
            model_surface_derivative(
                index,
                surfaces[side],
                pairs[side],
                u_step,
                true,
                space.domains[side],
                space.periods[side],
            )?,
            model_surface_derivative(
                index,
                surfaces[side],
                pairs[side],
                v_step,
                false,
                space.domains[side],
                space.periods[side],
            )?,
        ])
    });
    let [Some(first), Some(second)] = derivatives else {
        return None;
    };
    Some([
        [first[0].x, first[1].x, -second[0].x, -second[1].x],
        [first[0].y, first[1].y, -second[0].y, -second[1].y],
        [first[0].z, first[1].z, -second[0].z, -second[1].z],
    ])
}

pub(crate) fn clamp_intersection_parameters(
    parameters: &mut [f64; 4],
    space: IntersectionParameterSpace,
) {
    for side in 0..2 {
        let mut pair = Point2::new(parameters[side * 2], parameters[side * 2 + 1]);
        clamp_surface_parameters_with_periods(&mut pair, space.domains[side], space.periods[side]);
        parameters[side * 2] = pair.u;
        parameters[side * 2 + 1] = pair.v;
    }
}

pub(crate) fn clamp_surface_parameters_with_periods(
    parameters: &mut Point2,
    domain: Option<([f64; 2], [f64; 2])>,
    periods: [Option<f64>; 2],
) {
    if let Some((u_domain, v_domain)) = domain {
        if periods[0].is_none() {
            parameters.u = parameters.u.clamp(u_domain[0], u_domain[1]);
        }
        if periods[1].is_none() {
            parameters.v = parameters.v.clamp(v_domain[0], v_domain[1]);
        }
    }
}

pub(crate) fn determinant_3x3(matrix: [[f64; 3]; 3]) -> f64 {
    matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
        - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
        + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0])
}

pub(crate) fn null_vector_3x4(matrix: [[f64; 4]; 3]) -> Option<[f64; 4]> {
    let mut vector = [0.0; 4];
    for (omitted, component) in vector.iter_mut().enumerate() {
        let minor = std::array::from_fn(|row| {
            let mut column = 0;
            std::array::from_fn(|_| {
                while column == omitted {
                    column += 1;
                }
                let value = matrix[row][column];
                column += 1;
                value
            })
        });
        *component = if omitted % 2 == 0 { 1.0 } else { -1.0 } * determinant_3x3(minor);
    }
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    (norm.is_finite() && norm > 1.0e-14).then(|| vector.map(|value| value / norm))
}

pub(crate) fn solve_4x4(mut matrix: [[f64; 4]; 4], mut rhs: [f64; 4]) -> Option<[f64; 4]> {
    for pivot in 0..4 {
        let row = (pivot..4).max_by(|first, second| {
            matrix[*first][pivot]
                .abs()
                .total_cmp(&matrix[*second][pivot].abs())
        })?;
        if !matrix[row][pivot].is_finite() || matrix[row][pivot].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(pivot, row);
        rhs.swap(pivot, row);
        let pivot_row = matrix[pivot];
        for row in pivot + 1..4 {
            let factor = matrix[row][pivot] / matrix[pivot][pivot];
            for (value, pivot_value) in matrix[row][pivot..].iter_mut().zip(&pivot_row[pivot..]) {
                *value -= factor * pivot_value;
            }
            rhs[row] -= factor * rhs[pivot];
        }
    }
    let mut solution = [0.0; 4];
    for row in (0..4).rev() {
        let known = (row + 1..4)
            .map(|column| matrix[row][column] * solution[column])
            .sum::<f64>();
        solution[row] = (rhs[row] - known) / matrix[row][row];
    }
    solution
        .iter()
        .all(|value| value.is_finite())
        .then_some(solution)
}

/// Return a finite correction for a consistent rank-deficient linearization.
///
/// Tangential surface intersections can lose one first-order direction even
/// though the chart-selected nonlinear branch remains well defined. The
/// column-scaled diagonal term selects a bounded minimum-norm correction
/// without making the result depend on unlike support parameter units. The
/// caller still requires the corrected nonlinear surfaces to agree and to
/// remain inside the source chart tolerance, so this fallback cannot qualify a
/// nearby branch.
pub(crate) fn solve_damped_least_squares_4x4(
    matrix: [[f64; 4]; 4],
    rhs: [f64; 4],
) -> Option<[f64; 4]> {
    if !matrix.iter().flatten().all(|value| value.is_finite())
        || !rhs.iter().all(|value| value.is_finite())
    {
        return None;
    }
    let normal: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..4)
                .map(|index| matrix[index][row] * matrix[index][column])
                .sum::<f64>()
        })
    });
    let normal_rhs: [f64; 4] = std::array::from_fn(|column| {
        (0..4)
            .map(|index| matrix[index][column] * rhs[index])
            .sum::<f64>()
    });
    let max_column_scale = (0..4)
        .map(|index| normal[index][index].abs())
        .fold(0.0_f64, f64::max)
        .sqrt();
    if !max_column_scale.is_finite() || max_column_scale == 0.0 {
        return None;
    }
    let column_scales: [f64; 4] = std::array::from_fn(|index| {
        normal[index][index]
            .max(0.0)
            .sqrt()
            .max(max_column_scale * 1.0e-12)
    });
    let scaled_normal: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            normal[row][column] / (column_scales[row] * column_scales[column])
        })
    });
    let scaled_rhs: [f64; 4] =
        std::array::from_fn(|column| normal_rhs[column] / column_scales[column]);
    let initial_error = rhs.iter().map(|value| value * value).sum::<f64>();
    for exponent in -12..=-3 {
        let damping = 10_f64.powi(exponent);
        let mut regularized = scaled_normal;
        for (index, row) in regularized.iter_mut().enumerate() {
            row[index] += damping;
        }
        let Some(scaled_step) = solve_4x4(regularized, scaled_rhs) else {
            continue;
        };
        let step: [f64; 4] = std::array::from_fn(|index| scaled_step[index] / column_scales[index]);
        let linear_error = (0..4)
            .map(|row| {
                let residual = (0..4)
                    .map(|column| matrix[row][column] * step[column])
                    .sum::<f64>()
                    - rhs[row];
                residual * residual
            })
            .sum::<f64>();
        if linear_error.is_finite() && linear_error < initial_error {
            return Some(step);
        }
    }
    None
}

pub(crate) fn least_squares_step(
    du: Vector3,
    dv: Vector3,
    residual: Vector3,
) -> Option<(f64, f64)> {
    let du_squared = du.dot(du);
    let mixed = du.dot(dv);
    let dv_squared = dv.dot(dv);
    let determinant = du_squared * dv_squared - mixed * mixed;
    if !determinant.is_finite()
        || determinant.abs() <= f64::EPSILON * du_squared.max(dv_squared).powi(2)
    {
        return None;
    }
    let du_residual = du.dot(residual);
    let dv_residual = dv.dot(residual);
    Some((
        (dv_squared * du_residual - mixed * dv_residual) / determinant,
        (du_squared * dv_residual - mixed * du_residual) / determinant,
    ))
}

pub(crate) fn point_distance(first: Point3, second: Point3) -> f64 {
    (first.x - second.x)
        .hypot(first.y - second.y)
        .hypot(first.z - second.z)
}

pub(crate) fn intersection_side(
    ir: &CadIr,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    surface_xmt: u32,
    uv: Option<(&[[f64; 2]], &[f64])>,
) -> IntcurveSupportSide {
    let surface = surfaces_by_xmt.get(&surface_xmt).cloned();
    let pcurve = surface.as_ref().and_then(|surface_id| {
        let geometry = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| &candidate.id == surface_id)
            .map(|surface| &surface.geometry)?;
        let (uv, parameters) = uv?;
        if uv
            .iter()
            .flatten()
            .any(|value| missing_support_parameter(*value))
        {
            return None;
        }
        let control_points = uv
            .iter()
            .map(|pair| surface_parameters(geometry, *pair))
            .collect::<Option<Vec<_>>>()?;
        Some(PcurveGeometry::Nurbs {
            degree: 1,
            knots: linear_knots(parameters),
            control_points,
            weights: None,
            periodic: false,
        })
    });
    IntcurveSupportSide {
        surface,
        pcurve,
        pcurve_parameter_range: None,
    }
}

pub(crate) fn surface_parameters(surface: &SurfaceGeometry, uv: [f64; 2]) -> Option<Point2> {
    let point = match surface {
        SurfaceGeometry::Plane { .. } => Point2::new(uv[0] * 1000.0, uv[1] * 1000.0),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
            Point2::new(uv[0], uv[1] * 1000.0)
        }
        SurfaceGeometry::Sphere { .. }
        | SurfaceGeometry::Torus { .. }
        | SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => Point2::new(uv[0], uv[1]),
        SurfaceGeometry::Transformed { basis, .. } => return surface_parameters(basis, uv),
    };
    [point.u, point.v]
        .into_iter()
        .all(f64::is_finite)
        .then_some(point)
}

pub(crate) fn normalize_pcurve_parameters(
    pcurve: &mut PcurveGeometry,
    surface: &SurfaceGeometry,
) -> Option<()> {
    match pcurve {
        PcurveGeometry::Line { origin, direction } => {
            let end = Point2::new(origin.u + direction.u, origin.v + direction.v);
            let converted_origin = surface_parameters(surface, [origin.u, origin.v])?;
            let converted_end = surface_parameters(surface, [end.u, end.v])?;
            *origin = converted_origin;
            *direction = Point2::new(
                converted_end.u - converted_origin.u,
                converted_end.v - converted_origin.v,
            );
        }
        PcurveGeometry::Nurbs { control_points, .. } => {
            let converted = control_points
                .iter()
                .map(|point| surface_parameters(surface, [point.u, point.v]))
                .collect::<Option<Vec<_>>>()?;
            *control_points = converted;
        }
        _ => {}
    }
    Some(())
}
