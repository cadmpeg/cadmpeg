// SPDX-License-Identifier: Apache-2.0
//! Offset directrices in allocation-owned extrusion result charts.

use crate::checked::ExactUnitVector3;
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::units::UnitVector3;

use crate::families::consolidated::records::class25::{self, Class25SourceRelation};
use crate::wire::bytes::{allocation_reference, AllocationReference};

use super::super::rolling_ball_patch::fit_edge_with_error;
use super::near;

const DIRECTRIX_FIT_MM: f64 = 0.00015;
const TANGENT_STEP: f64 = 0.001;
const BOUND_AGREEMENT: f64 = 1.0e-9;

pub(super) struct Extrusion {
    pub(super) directrix: AllocationReference,
    direction: UnitVector3,
    bounds: [[f64; 2]; 2],
}

impl Extrusion {
    pub(super) fn read(bytes: &[u8], bounds: [[f64; 2]; 2]) -> Option<Self> {
        let mut at = 0;
        let directrix = allocation_reference(bytes, &mut at)?;
        let mut view = View::over_retained(bytes.get(at..)?);
        let direction = direction(&mut view)?;
        let linear = interval(&mut view)?;
        if !linear.into_iter().zip(bounds[1]).all(|(a, b)| near(a, b))
            || view.f64_le()? != 1.0
            || view.f64_le()? != 0.0
        {
            return None;
        }
        let limits = [view.f64_le()?, view.f64_le()?];
        let indices = view.take(2)?;
        if !limits.iter().all(|v| v.is_finite() && *v >= 0.0)
            || !indices.iter().all(|index| *index % 4 == 1)
            || !view.is_empty()
        {
            return None;
        }
        Some(Self {
            directrix,
            direction,
            bounds,
        })
    }

    pub(super) fn offset(&self, bytes: &[u8]) -> Option<(AllocationReference, DirectrixOffset)> {
        let mut at = 0;
        let source = allocation_reference(bytes, &mut at)?;
        let mut view = View::over_retained(bytes.get(at..)?);
        let from = interval(&mut view)?;
        if view.u8()? != 0x05 {
            return None;
        }
        let distance = view.f64_le()?;
        let normal = direction(&mut view)?;
        let result = interval(&mut view)?;
        if !distance.is_finite()
            || distance == 0.0
            || !view.is_empty()
            || normal != self.direction
            || !result
                .into_iter()
                .zip(self.bounds[0])
                .all(|(a, b)| near(a, b))
            || result[1] - result[0] > from[1] - from[0] + 2.0 * BOUND_AGREEMENT
        {
            return None;
        }
        Some((
            source,
            DirectrixOffset {
                from,
                result,
                distance,
            },
        ))
    }

    pub(super) fn expanded_to_include(&self, bounds: [[f64; 2]; 2]) -> Self {
        Self {
            directrix: self.directrix,
            direction: self.direction,
            bounds: std::array::from_fn(|axis| {
                [
                    self.bounds[axis][0].min(bounds[axis][0]),
                    self.bounds[axis][1].max(bounds[axis][1]),
                ]
            }),
        }
    }

    pub(super) fn extends_source(&self, offset: &DirectrixOffset) -> bool {
        let [lo, hi] = self.bounds[0].map(|t| offset.source_parameter(t));
        lo < offset.from[0] - BOUND_AGREEMENT || hi > offset.from[1] + BOUND_AGREEMENT
    }

    pub(super) fn surface(
        &self,
        offset: &DirectrixOffset,
        knot_domain: [f64; 2],
        point: impl Fn(f64) -> Option<Point3>,
    ) -> Option<NurbsSurface> {
        let domain = if self.extends_source(offset) {
            knot_domain
        } else {
            offset.from
        };
        let source_bounds = self.bounds[0].map(|t| offset.source_parameter(t));
        if !source_bounds.iter().all(|t| t.is_finite())
            || source_bounds[0] < knot_domain[0] - BOUND_AGREEMENT
            || source_bounds[1] > knot_domain[1] + BOUND_AGREEMENT
            || source_bounds[0] < domain[0] - BOUND_AGREEMENT
            || source_bounds[1] > domain[1] + BOUND_AGREEMENT
        {
            return None;
        }
        let axis = *self.direction.as_raw();
        let curve = fit_edge_with_error(
            self.bounds[0],
            |t| {
                let t = offset.source_parameter(t).clamp(domain[0], domain[1]);
                let a = point((t - TANGENT_STEP).max(domain[0]))?;
                let b = point((t + TANGENT_STEP).min(domain[1]))?;
                let unit =
                    UnitVector3::normalized_by_largest_component(b.vector_from(a).cross(axis))?;
                let p = point(t)?.translated(*unit.as_raw(), offset.distance);
                Some([p.x, p.y, p.z])
            },
            DIRECTRIX_FIT_MM,
        )?;
        let [lo, hi] = self.bounds[1];
        let rows = curve
            .control_points()
            .iter()
            .map(|p| {
                let p = p.get();
                [lo, hi].map(|t| p.translated(axis, t)).to_vec()
            })
            .collect();
        NurbsSurface::new(
            NurbsSurfaceAxis::new(curve.degree(), curve.knots().clone(), false),
            NurbsSurfaceAxis::new(1, vec![lo, lo, hi, hi], false),
            NurbsPoleGrid::Polynomial { rows },
            false,
        )
        .ok()
    }
}

pub(super) struct DirectrixOffset {
    pub(super) from: [f64; 2],
    pub(super) result: [f64; 2],
    distance: f64,
}

impl DirectrixOffset {
    pub(super) fn maps_result(&self) -> bool {
        self.result[0] < self.from[0] - BOUND_AGREEMENT
            || self.result[1] > self.from[1] + BOUND_AGREEMENT
    }

    fn source_parameter(&self, result_parameter: f64) -> f64 {
        if !self.maps_result() {
            return result_parameter;
        }
        let fraction = (result_parameter - self.result[0]) / (self.result[1] - self.result[0]);
        (self.from[1] - self.from[0]).mul_add(fraction, self.from[0])
    }
}

pub(super) fn identity_source(bytes: &[u8], from: [f64; 2]) -> Option<[AllocationReference; 2]> {
    let map = class25::parse(bytes)?;
    let Class25SourceRelation::Affine {
        range,
        scale,
        offset,
    } = map.source
    else {
        return None;
    };
    if scale.get() != 1.0
        || offset.get() != 0.0
        || range.lower() > from[0]
        || range.upper() < from[1]
    {
        return None;
    }
    Some(
        [map.source_operand, map.target_operand].map(|operand| AllocationReference {
            value: operand.value,
            encoding: operand.encoding,
        }),
    )
}

fn direction(view: &mut View<'_>) -> Option<UnitVector3> {
    let xyz = [view.f64_le()?, view.f64_le()?, view.f64_le()?];
    ExactUnitVector3::new(xyz).map(Into::into)
}

fn interval(view: &mut View<'_>) -> Option<[f64; 2]> {
    Some(IncreasingParameterInterval::new([view.f64_le()?, view.f64_le()?])?.endpoints())
}

#[cfg(test)]
mod tests;
