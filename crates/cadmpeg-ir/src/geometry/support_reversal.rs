// SPDX-License-Identifier: Apache-2.0
//! Support contexts of a curve whose solved parameter `t` is replaced by `-t`.

use super::pcurve::{LinePcurve, PcurveGeometry};
use super::{
    CacheFirstCurveParameterization, DirectedParameterRange, IntcurveSupportContext,
    IntcurveSupportSide, ProceduralCurveDefinition, RevisionCacheForm, SupportPcurve,
    SurfaceCurveFamily, SurfaceCurveTail,
};
use crate::math::Point2;
use crate::scalar::FiniteReal;
use crate::topology::ParameterInterval;

impl ProceduralCurveDefinition {
    /// Restate every support context of this definition on the reversed
    /// solved parameter `-t`, as its reversed solved cache is.
    ///
    /// `None` when a context holds a pcurve that has no reversed form; the
    /// definition is then unchanged. A definition without a support context
    /// is unchanged.
    pub fn reverse_support_contexts(&mut self) -> Option<()> {
        match self {
            Self::Intersection { context, .. } => *context = context.reversed_about_zero()?,
            Self::SurfaceCurve { family } => family.reverse_about_zero()?,
            _ => {}
        }
        Some(())
    }
}

impl SurfaceCurveFamily {
    fn reverse_about_zero(&mut self) -> Option<()> {
        let context = self.context().reversed_about_zero()?;
        match self {
            Self::Blend { context: own, tail }
            | Self::SurfaceConstrained { context: own, tail }
            | Self::Skin { context: own, tail } => {
                let form = tail.as_ref().map(|tail| tail.form.reversed_about_zero());
                *own = context;
                if let (Some(tail), Some(form)) = (tail.as_mut(), form) {
                    tail.form = form;
                }
            }
            Self::Parametric { context: own, tail } => {
                let form = tail.as_ref().map(|tail| tail.form.reversed_about_zero());
                *own = context;
                if let (Some(tail), Some(form)) = (tail.as_mut(), form) {
                    tail.form = form;
                }
            }
        }
        Some(())
    }
}

impl SurfaceCurveTail {
    fn reversed_about_zero(&self) -> Self {
        let mut reversed = self.clone();
        reversed.solved_range = reversed_bounds(self.solved_range);
        if let RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval,
            ..
        }) = &mut reversed.cache
        {
            *interval = reversed_bounds(*interval);
        }
        reversed
    }
}

impl IntcurveSupportContext {
    /// This context on the reversed solved parameter `-t`: the interval
    /// `[t0, t1]` becomes `[-t1, -t0]`, each discontinuity array is negated
    /// in reversed order, and each pcurve is traversed as `-t`.
    ///
    /// `None` when a pcurve has no reversed form.
    #[must_use]
    pub fn reversed_about_zero(&self) -> Option<Self> {
        let [start, end] = self.parameter_range.endpoints();
        let parameter_range = ParameterInterval::new([-end, -start]).ok()?;
        let sides = [
            reversed_side(&self.sides[0])?,
            reversed_side(&self.sides[1])?,
        ];
        let discontinuities = self
            .discontinuities
            .clone()
            .map(|values| values.into_iter().rev().map(FiniteReal::negated).collect());
        Some(Self {
            sides,
            parameter_range,
            discontinuities,
        })
    }
}

fn reversed_side(side: &IntcurveSupportSide) -> Option<IntcurveSupportSide> {
    let pcurve = match &side.pcurve {
        None => None,
        Some(pcurve) => Some(SupportPcurve {
            geometry: reversed_pcurve(&pcurve.geometry)?,
            parameter_range: match pcurve.parameter_range {
                None => None,
                Some(range) => {
                    let [start, end] = range.endpoints();
                    Some(DirectedParameterRange::new([-end, -start]).ok()?)
                }
            },
        }),
    };
    Some(IntcurveSupportSide {
        surface: side.surface.clone(),
        pcurve,
    })
}

/// The same parameter-space curve traversed as `-t`.
fn reversed_pcurve(geometry: &PcurveGeometry) -> Option<PcurveGeometry> {
    Some(match geometry {
        PcurveGeometry::Nurbs { nurbs } => {
            let mut nurbs = nurbs.clone();
            nurbs.reverse_parameterization();
            PcurveGeometry::Nurbs { nurbs }
        }
        PcurveGeometry::Line(line) => {
            let direction = line.direction().as_raw();
            PcurveGeometry::Line(
                LinePcurve::try_new(line.origin().get(), Point2::new(-direction.u, -direction.v))
                    .ok()?,
            )
        }
        PcurveGeometry::Ellipse(ellipse) => PcurveGeometry::Ellipse(ellipse.reversed_about_zero()),
        PcurveGeometry::Parabola(parabola) => {
            PcurveGeometry::Parabola(parabola.reversed_about_zero())
        }
        PcurveGeometry::Hyperbola(hyperbola) => {
            PcurveGeometry::Hyperbola(hyperbola.reversed_about_zero())
        }
        _ => return None,
    })
}

fn reversed_bounds(bounds: [Option<FiniteReal>; 2]) -> [Option<FiniteReal>; 2] {
    let [start, end] = bounds;
    [end.map(FiniteReal::negated), start.map(FiniteReal::negated)]
}

#[cfg(test)]
mod tests;
