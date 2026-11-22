// SPDX-License-Identifier: Apache-2.0
//! Range guards for deferred curve speed bounds.

use cadmpeg_ir::eval::nurbs_curve_parameter_domain;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;

const SPEED_DEGREE_UPPER: u32 = 32;
const SPEED_COORDINATE_MAGNITUDE: f64 = 1e50;
const SPEED_KNOT_WIDTH_LOWER: f64 = 1e-50;
const SPEED_WEIGHT_LOWER: f64 = 1e-10;
const SPEED_WEIGHT_UPPER: f64 = 1e10;
const SPEED_COMPONENT_LOWER: f64 = 1e-100;
const SPEED_COORDINATE_SEPARATION: f64 = 2.938_735_877_055_719e-39;

pub(super) struct FiniteSpeed<'curve> {
    curve: &'curve NurbsCurve,
    scale: f64,
    separated: bool,
}

impl<'curve> FiniteSpeed<'curve> {
    pub(super) fn new(curve: &'curve NurbsCurve) -> Option<Self> {
        let degree = usize::try_from(curve.degree()).ok()?;
        let count = curve.pole_count();
        if !(1..=SPEED_DEGREE_UPPER).contains(&curve.degree())
            || curve.knots().len() != count.checked_add(degree)?.checked_add(1)?
            || nurbs_curve_parameter_domain(curve).is_none()
            || curve
                .knots()
                .iter()
                .any(|knot| knot.abs() > SPEED_COORDINATE_MAGNITUDE)
        {
            return None;
        }
        for pair in curve.knots().windows(2) {
            let width = pair[1] - pair[0];
            if width < 0.0 || (width > 0.0 && width < SPEED_KNOT_WIDTH_LOWER) {
                return None;
            }
        }
        let mut scale = 0.0_f64;
        let mut separated = true;
        for index in 0..count {
            let point = curve.pole_rows().point_at(index)?.get();
            let coordinates: [f64; 3] = point.into();
            let weight = curve.pole_rows().weight_at(index).unwrap_or(1.0);
            if coordinates
                .iter()
                .any(|value| value.abs() > SPEED_COORDINATE_MAGNITUDE)
                || !(SPEED_WEIGHT_LOWER..=SPEED_WEIGHT_UPPER).contains(&weight)
            {
                return None;
            }
            scale = scale.max(weight);
            separated &= coordinates.iter().all(|value| coordinate_separated(*value));
        }
        Some(Self {
            curve,
            scale,
            separated,
        })
    }

    pub(super) fn admits(&self, origin: Point3) -> bool {
        let origin: [f64; 3] = origin.into();
        if origin
            .iter()
            .any(|value| !value.is_finite() || value.abs() > SPEED_COORDINATE_MAGNITUDE)
        {
            return false;
        }
        if self.separated && origin.iter().all(|value| coordinate_separated(*value)) {
            return true;
        }
        self.components_admitted(origin)
    }

    fn components_admitted(&self, origin: [f64; 3]) -> bool {
        let curve = self.curve;
        let mut previous: Option<[f64; 4]> = None;
        for index in 0..curve.pole_count() {
            let Some(point) = curve.pole_rows().point_at(index) else {
                return false;
            };
            let point: [f64; 3] = point.get().into();
            let weight = curve.pole_rows().weight_at(index).unwrap_or(1.0) / self.scale;
            let relative: [f64; 3] = std::array::from_fn(|axis| point[axis] - origin[axis]);
            let weighted: [f64; 3] = std::array::from_fn(|axis| weight * relative[axis]);
            if relative
                .iter()
                .zip(weighted)
                .any(|(raw, product)| *raw != 0.0 && product == 0.0)
            {
                return false;
            }
            let homogeneous = [weighted[0], weighted[1], weighted[2], weight];
            if homogeneous.iter().any(|value| !component_admitted(*value))
                || previous.is_some_and(|previous| {
                    homogeneous
                        .iter()
                        .zip(previous)
                        .any(|(next, previous)| !component_admitted(*next - previous))
                })
            {
                return false;
            }
            previous = Some(homogeneous);
        }
        true
    }
}

fn component_admitted(value: f64) -> bool {
    value == 0.0 || (value.is_finite() && value.abs() >= SPEED_COMPONENT_LOWER)
}

fn coordinate_separated(value: f64) -> bool {
    value == 0.0 || value.abs() >= SPEED_COORDINATE_SEPARATION
}

#[cfg(test)]
mod tests;
