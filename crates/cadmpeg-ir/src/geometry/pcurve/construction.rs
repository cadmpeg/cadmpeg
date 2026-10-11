// SPDX-License-Identifier: Apache-2.0
//! Shared construction algorithms with an explicit storage and work policy.

use super::{
    PcurveNurbs, PcurveNurbsPoles, PolarNurbsPole, PolarNurbsPoles, PolarPcurveNurbs,
    WeightedPolarNurbsPole, WeightedPole2,
};
use crate::geometry::nurbs::{
    admit_knots, require_curve_cardinality, require_weight_lane, KnotValue, NurbsAdmission,
    PoleValue,
};
use crate::scalar::{FiniteReal, NonZeroReal};
use crate::units::FinitePoint2;
use cadmpeg_core::decode::ScopedReservation;

fn pair_weights<P, W, T, A: NurbsAdmission>(
    admission: &A,
    points: Vec<P>,
    weights: Vec<W>,
    storage: &mut Option<ScopedReservation<'_>>,
    operation: &'static str,
    mut row: impl FnMut(usize, P, W) -> Result<T, A::Error>,
) -> Result<Vec<T>, A::Error> {
    let mut output = Vec::new();
    let points = admission.admit_iter(points, operation)?;
    let weights = admission.admit_iter(weights, operation)?;
    for (index, (point, value)) in points.zip(weights).enumerate() {
        admission.reserve(&mut output, storage, operation)?;
        output.push(row(index, point, value)?);
    }
    Ok(output)
}

pub(super) fn pair_pcurve_lanes<P, W, A: NurbsAdmission>(
    admission: &A,
    points: Vec<P>,
    weights: Option<Vec<W>>,
    storage: &mut Option<ScopedReservation<'_>>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, A::Error>,
) -> Result<PcurveNurbsPoles<P>, A::Error> {
    let Some(weights) = weights else {
        return Ok(PcurveNurbsPoles::Polynomial { points });
    };
    require_weight_lane(admission, "pcurve poles", points.len(), weights.len())?;
    Ok(PcurveNurbsPoles::Rational {
        points: pair_weights(
            admission,
            points,
            weights,
            storage,
            "IR pcurve paired poles",
            |index, point, value| {
                Ok(WeightedPole2 {
                    point,
                    weight: weight(index, value)?,
                })
            },
        )?,
    })
}

pub(super) fn pair_polar_lanes<P, S, W, A: NurbsAdmission>(
    admission: &A,
    poles: Vec<PolarNurbsPole<P, S>>,
    weights: Option<Vec<W>>,
    storage: &mut Option<ScopedReservation<'_>>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, A::Error>,
) -> Result<PolarNurbsPoles<P, S>, A::Error> {
    let Some(weights) = weights else {
        return Ok(PolarNurbsPoles::Polynomial { poles });
    };
    require_weight_lane(admission, "polar poles", poles.len(), weights.len())?;
    Ok(PolarNurbsPoles::Rational {
        poles: pair_weights(
            admission,
            poles,
            weights,
            storage,
            "IR polar paired poles",
            |index, pole, value| {
                Ok(WeightedPolarNurbsPole {
                    radial: pole.radial,
                    axial: pole.axial,
                    weight: weight(index, value)?,
                })
            },
        )?,
    })
}

fn map_pcurve_pole<P: PoleValue<FinitePoint2>, A: NurbsAdmission>(
    admission: &A,
    point: P,
) -> Result<FinitePoint2, A::Error> {
    match point.admit() {
        Some(point) => Ok(point),
        None => {
            Err(admission.structure(format_args!("control_points contains a non-finite point"))?)
        }
    }
}

fn map_pcurve_poles<P: PoleValue<FinitePoint2>, A: NurbsAdmission>(
    admission: &A,
    poles: PcurveNurbsPoles<P>,
) -> Result<PcurveNurbsPoles<FinitePoint2>, A::Error> {
    Ok(match poles {
        PcurveNurbsPoles::Polynomial { points } => PcurveNurbsPoles::Polynomial {
            points: admission.collect(points, "IR pcurve admitted poles", |point| {
                map_pcurve_pole(admission, point)
            })?,
        },
        PcurveNurbsPoles::Rational { points } => PcurveNurbsPoles::Rational {
            points: admission.collect(points, "IR pcurve admitted poles", |pole| {
                Ok(WeightedPole2 {
                    point: map_pcurve_pole(admission, pole.point)?,
                    weight: pole.weight,
                })
            })?,
        },
    })
}

pub(super) fn build_pcurve<P: PoleValue<FinitePoint2>, K: KnotValue, A: NurbsAdmission>(
    admission: &A,
    degree: u32,
    knots: K,
    poles: PcurveNurbsPoles<P>,
    periodic: bool,
) -> Result<PcurveNurbs, A::Error> {
    require_curve_cardinality(
        admission,
        degree,
        knots.knot_count(),
        poles.count(),
        "control_points",
    )?;
    if degree == 0 {
        return Err(admission.structure(format_args!("pcurve NURBS degree must be positive"))?);
    }
    let poles = P::admit_pcurve_poles(poles, |poles| map_pcurve_poles(admission, poles))?;
    let knots = admit_knots(admission, knots, "")?;
    Ok(PcurveNurbs {
        degree,
        knots,
        poles,
        periodic,
    })
}

fn map_polar_pole<P: PoleValue<FinitePoint2>, S: PoleValue<FiniteReal>, A: NurbsAdmission>(
    admission: &A,
    radial: P,
    axial: S,
) -> Result<(FinitePoint2, FiniteReal), A::Error> {
    match radial.admit().zip(axial.admit()) {
        Some(pole) => Ok(pole),
        None => Err(admission.structure(format_args!("poles contain a non-finite value"))?),
    }
}

pub(super) fn build_polar<
    P: PoleValue<FinitePoint2>,
    S: PoleValue<FiniteReal>,
    K: KnotValue,
    A: NurbsAdmission,
>(
    admission: &A,
    degree: u32,
    knots: K,
    poles: PolarNurbsPoles<P, S>,
    periodic: bool,
) -> Result<PolarPcurveNurbs, A::Error> {
    require_curve_cardinality(
        admission,
        degree,
        knots.knot_count(),
        poles.count(),
        "poles",
    )?;
    if degree == 0 {
        return Err(admission.structure(format_args!("polar NURBS degree must be positive"))?);
    }
    let poles = match poles {
        PolarNurbsPoles::Polynomial { poles } => PolarNurbsPoles::Polynomial {
            poles: admission.collect(poles, "IR polar admitted poles", |pole| {
                let (radial, axial) = map_polar_pole(admission, pole.radial, pole.axial)?;
                Ok(PolarNurbsPole { radial, axial })
            })?,
        },
        PolarNurbsPoles::Rational { poles } => PolarNurbsPoles::Rational {
            poles: admission.collect(poles, "IR polar admitted poles", |pole| {
                let (radial, axial) = map_polar_pole(admission, pole.radial, pole.axial)?;
                Ok(WeightedPolarNurbsPole {
                    radial,
                    axial,
                    weight: pole.weight,
                })
            })?,
        },
    };
    let knots = admit_knots(admission, knots, "")?;
    Ok(PolarPcurveNurbs {
        degree,
        knots,
        poles,
        periodic,
    })
}

#[cfg(test)]
mod tests;
