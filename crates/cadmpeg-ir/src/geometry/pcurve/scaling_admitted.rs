// SPDX-License-Identifier: Apache-2.0
//! Coordinate scaling of owned, admitted pcurve carriers.

use super::{
    HarmonicPcurve, HyperbolicPcurve, LinePcurve, ParabolaPcurve, PcurveCoordinateScaleError,
    PcurveGeometry, PcurveNurbsPoles,
};
use crate::math::Point2;
use crate::scalar::{FiniteReal, NonZeroReal};
use crate::transform::Transform2;
use crate::units::FinitePoint2;
use cadmpeg_core::decode::admission::{Admission, StandardAdmission};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

enum ScalingError<E> {
    Resource(E),
    Geometry(&'static str),
}
impl<E> From<&'static str> for ScalingError<E> {
    fn from(error: &'static str) -> Self {
        Self::Geometry(error)
    }
}

impl PcurveGeometry {
    /// Scale chart coordinates atomically without changing the curve parameterization.
    pub fn try_scale_coordinates(
        &mut self,
        scales: [f64; 2],
    ) -> Result<(), PcurveCoordinateScaleError> {
        let mut candidate = self.clone();
        match scale_in_place(&StandardAdmission, &mut candidate, scales) {
            Ok(()) => {
                *self = candidate;
                Ok(())
            }
            Err(ScalingError::Geometry(error)) => {
                Err(PcurveCoordinateScaleError::Invalid(error.to_owned()))
            }
            Err(ScalingError::Resource(error)) => match error {},
        }
    }

    /// Scale owned chart coordinates without copying basis boxes or pole rows.
    /// Resource refusals stay separate from geometric refusals. Refused candidates are consumed.
    pub fn scaled_coordinates_owned(
        mut self,
        ctx: &DecodeContext<'_>,
        scales: [f64; 2],
    ) -> Result<Result<Self, &'static str>, CodecError> {
        match scale_in_place(ctx, &mut self, scales) {
            Ok(()) => Ok(Ok(self)),
            Err(ScalingError::Geometry(error)) => Ok(Err(error)),
            Err(ScalingError::Resource(error)) => Err(error),
        }
    }
}

fn scale_pole(point: &mut FinitePoint2, [u_scale, v_scale]: [f64; 2]) -> Result<(), &'static str> {
    let raw = point.get();
    *point = FinitePoint2::new(Point2::new(raw.u * u_scale, raw.v * v_scale))
        .ok_or("control_points contains a non-finite point")?;
    Ok(())
}

fn scale_in_place<A: Admission>(
    admission: &A,
    geometry: &mut PcurveGeometry,
    scales: [f64; 2],
) -> Result<(), ScalingError<A::Error>> {
    admission.charge_work(0, "IR pcurve coordinate scaling work")
        .map_err(ScalingError::Resource)?;
    let [u_scale, v_scale] = scales;
    let scale = |point: Point2| Point2::new(point.u * u_scale, point.v * v_scale);
    let isotropic = u_scale == v_scale;
    let scaled = match geometry {
        PcurveGeometry::Line(line) => PcurveGeometry::Line(LinePcurve::try_new(
            scale(*line.origin().as_raw()),
            scale(*line.direction().as_raw()),
        )?),
        PcurveGeometry::Circle(circle) if isotropic => {
            PcurveGeometry::Circle(circle.scaled_isotropic(u_scale)?)
        }
        PcurveGeometry::Circle(circle) => PcurveGeometry::Harmonic(HarmonicPcurve::try_new(
            scale(circle.center().get()),
            scale(Point2::new(
                circle.radius().get() * circle.x_axis().u,
                circle.radius().get() * circle.x_axis().v,
            )),
            scale(Point2::new(
                circle.radius().get() * circle.y_axis().u,
                circle.radius().get() * circle.y_axis().v,
            )),
        )?),
        PcurveGeometry::Ellipse(ellipse) if isotropic => {
            PcurveGeometry::Ellipse(ellipse.scaled_isotropic(u_scale)?)
        }
        PcurveGeometry::Ellipse(ellipse) => PcurveGeometry::Harmonic(HarmonicPcurve::try_new(
            scale(ellipse.center().get()),
            scale(Point2::new(
                ellipse.major_radius().get() * ellipse.x_axis().u,
                ellipse.major_radius().get() * ellipse.x_axis().v,
            )),
            scale(Point2::new(
                ellipse.minor_radius().get() * ellipse.y_axis().u,
                ellipse.minor_radius().get() * ellipse.y_axis().v,
            )),
        )?),
        PcurveGeometry::Parabola(parabola) => PcurveGeometry::Parabola(ParabolaPcurve::try_new(
            scale(parabola.vertex().get()),
            scale(parabola.x_axis().get()),
            scale(parabola.y_axis().get()),
            parabola.focal_distance().get(),
        )?),
        PcurveGeometry::Hyperbola(hyperbola) if isotropic => {
            PcurveGeometry::Hyperbola(hyperbola.scaled_isotropic(u_scale)?)
        }
        PcurveGeometry::Hyperbola(hyperbola) => {
            PcurveGeometry::Hyperbolic(HyperbolicPcurve::try_new(
                scale(hyperbola.center().get()),
                scale(Point2::new(
                    hyperbola.major_radius().get() * hyperbola.x_axis().u,
                    hyperbola.major_radius().get() * hyperbola.x_axis().v,
                )),
                scale(Point2::new(
                    hyperbola.minor_radius().get() * hyperbola.y_axis().u,
                    hyperbola.minor_radius().get() * hyperbola.y_axis().v,
                )),
            )?)
        }
        PcurveGeometry::Harmonic(harmonic) => PcurveGeometry::Harmonic(HarmonicPcurve::try_new(
            scale(harmonic.center().get()),
            scale(harmonic.cosine().get()),
            scale(harmonic.sine().get()),
        )?),
        PcurveGeometry::Hyperbolic(hyperbolic) => {
            PcurveGeometry::Hyperbolic(HyperbolicPcurve::try_new(
                scale(hyperbolic.center().get()),
                scale(hyperbolic.cosine().get()),
                scale(hyperbolic.sine().get()),
            )?)
        }
        PcurveGeometry::Nurbs { nurbs } => {
            match &mut nurbs.poles {
                PcurveNurbsPoles::Polynomial { points } => {
                    let mut points = points.iter_mut();
                    loop {
                        admission.charge_work(1, "IR pcurve pole coordinate scaling work")
                            .map_err(ScalingError::Resource)?;
                        let Some(point) = points.next() else { break; };
                        scale_pole(point, scales)?;
                    }
                }
                PcurveNurbsPoles::Rational { points } => {
                    let mut points = points.iter_mut();
                    loop {
                        admission.charge_work(1, "IR pcurve pole coordinate scaling work")
                            .map_err(ScalingError::Resource)?;
                        let Some(pole) = points.next() else { break; };
                        scale_pole(&mut pole.point, scales)?;
                    }
                }
            }
            return Ok(());
        }
        PcurveGeometry::Trimmed(trimmed) => {
            admission.charge_work(1, "IR pcurve coordinate scaling work")
                .map_err(ScalingError::Resource)?;
            let _depth = admission.enter_nested("IR pcurve coordinate scaling nesting")
                .map_err(ScalingError::Resource)?;
            scale_in_place(admission, &mut trimmed.basis, scales)?;
            return Ok(());
        }
        PcurveGeometry::Offset(offset) => {
            if !isotropic {
                return Err(ScalingError::Geometry(
                    "offset coordinate scaling must be isotropic",
                ));
            }
            admission.charge_work(1, "IR pcurve coordinate scaling work")
                .map_err(ScalingError::Resource)?;
            let _depth = admission.enter_nested("IR pcurve coordinate scaling nesting")
                .map_err(ScalingError::Resource)?;
            scale_in_place(admission, &mut offset.basis, scales)?;
            offset.distance = FiniteReal::new(offset.distance.get() * u_scale)
                .ok_or("OffsetPcurve.distance must be finite")?;
            return Ok(());
        }
        PcurveGeometry::Transformed(placed) => {
            let u_scale = NonZeroReal::new(u_scale)
                .ok_or("transformed pcurve coordinate scales must be finite and nonzero")?;
            let v_scale = NonZeroReal::new(v_scale)
                .ok_or("transformed pcurve coordinate scales must be finite and nonzero")?;
            let mut rows = placed.transform.affine_rows();
            rows[0][1] *= u_scale.get() / v_scale.get();
            rows[0][2] *= u_scale.get();
            rows[1][0] *= v_scale.get() / u_scale.get();
            rows[1][2] *= v_scale.get();
            let transform = Transform2::affine(rows).ok_or("scaled pcurve transform is invalid")?;
            admission.charge_work(1, "IR pcurve coordinate scaling work")
                .map_err(ScalingError::Resource)?;
            let _depth = admission.enter_nested("IR pcurve coordinate scaling nesting")
                .map_err(ScalingError::Resource)?;
            scale_in_place(admission, &mut placed.basis, scales)?;
            placed.transform = transform;
            return Ok(());
        }
        PcurveGeometry::PolarHarmonic(_)
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle(_) => {
            return if isotropic && u_scale == 1.0 {
                Ok(())
            } else {
                Err(ScalingError::Geometry(
                    "polar pcurve coordinate scaling must be identity",
                ))
            };
        }
    };
    *geometry = scaled;
    Ok(())
}

#[cfg(test)]
mod tests;
