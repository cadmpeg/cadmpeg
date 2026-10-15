// SPDX-License-Identifier: Apache-2.0
//! Pole pairing and construction shared by the parameter-space NURBS carriers.

use super::{
    PcurveNurbs, PcurveNurbsPoles, PolarNurbsPole, PolarNurbsPoles, PolarPcurveNurbs,
    WeightedPolarNurbsPole, WeightedPole2,
};
use crate::geometry::nurbs::{
    admit_knots, require_curve_cardinality, require_weight_lane, KnotValue, NurbsError, PoleValue,
};
use crate::scalar::{FiniteReal, NonZeroReal};
use crate::units::FinitePoint2;

fn pair_weights<P, W, T, E>(
    points: Vec<P>,
    weights: Vec<W>,
    mut row: impl FnMut(usize, P, W) -> Result<T, E>,
) -> Result<Vec<T>, E> {
    points
        .into_iter()
        .zip(weights)
        .enumerate()
        .map(|(index, (point, value))| row(index, point, value))
        .collect()
}

pub(super) fn pair_pcurve_lanes<P, W>(
    points: Vec<P>,
    weights: Option<Vec<W>>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, NurbsError>,
) -> Result<PcurveNurbsPoles<P>, NurbsError> {
    let Some(weights) = weights else {
        return Ok(PcurveNurbsPoles::Polynomial { points });
    };
    require_weight_lane("pcurve poles", points.len(), weights.len())?;
    Ok(PcurveNurbsPoles::Rational {
        points: pair_weights(points, weights, |index, point, value| {
            Ok::<_, NurbsError>(WeightedPole2 {
                point,
                weight: weight(index, value)?,
            })
        })?,
    })
}

pub(super) fn pair_polar_lanes<P, S, W>(
    poles: Vec<PolarNurbsPole<P, S>>,
    weights: Option<Vec<W>>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, NurbsError>,
) -> Result<PolarNurbsPoles<P, S>, NurbsError> {
    let Some(weights) = weights else {
        return Ok(PolarNurbsPoles::Polynomial { poles });
    };
    require_weight_lane("polar poles", poles.len(), weights.len())?;
    Ok(PolarNurbsPoles::Rational {
        poles: pair_weights(poles, weights, |index, pole, value| {
            Ok::<_, NurbsError>(WeightedPolarNurbsPole {
                radial: pole.radial,
                axial: pole.axial,
                weight: weight(index, value)?,
            })
        })?,
    })
}

fn map_pcurve_pole<P: PoleValue<FinitePoint2>>(point: P) -> Result<FinitePoint2, NurbsError> {
    match point.admit() {
        Some(point) => Ok(point),
        None => Err(NurbsError::Structure(
            "control_points contains a non-finite point".to_owned(),
        )),
    }
}

fn map_pcurve_poles<P: PoleValue<FinitePoint2>>(
    poles: PcurveNurbsPoles<P>,
) -> Result<PcurveNurbsPoles<FinitePoint2>, NurbsError> {
    Ok(match poles {
        PcurveNurbsPoles::Polynomial { points } => PcurveNurbsPoles::Polynomial {
            points: points
                .into_iter()
                .map(|point| map_pcurve_pole(point))
                .collect::<Result<Vec<_>, NurbsError>>()?,
        },
        PcurveNurbsPoles::Rational { points } => PcurveNurbsPoles::Rational {
            points: points
                .into_iter()
                .map(|pole| {
                    Ok(WeightedPole2 {
                        point: map_pcurve_pole(pole.point)?,
                        weight: pole.weight,
                    })
                })
                .collect::<Result<Vec<_>, NurbsError>>()?,
        },
    })
}

pub(super) fn build_pcurve<P: PoleValue<FinitePoint2>, K: KnotValue>(
    degree: u32,
    knots: K,
    poles: PcurveNurbsPoles<P>,
    periodic: bool,
) -> Result<PcurveNurbs, NurbsError> {
    require_curve_cardinality(degree, knots.knot_count(), poles.count(), "control_points")?;
    if degree == 0 {
        return Err(NurbsError::Structure(
            "pcurve NURBS degree must be positive".to_owned(),
        ));
    }
    let poles = P::admit_pcurve_poles(poles, |poles| map_pcurve_poles(poles))?;
    let knots = admit_knots(knots, "")?;
    Ok(PcurveNurbs {
        degree,
        knots,
        poles,
        periodic,
    })
}

fn map_polar_pole<P: PoleValue<FinitePoint2>, S: PoleValue<FiniteReal>>(
    radial: P,
    axial: S,
) -> Result<(FinitePoint2, FiniteReal), NurbsError> {
    match radial.admit().zip(axial.admit()) {
        Some(pole) => Ok(pole),
        None => Err(NurbsError::Structure(
            "poles contain a non-finite value".to_owned(),
        )),
    }
}

pub(super) fn build_polar<P: PoleValue<FinitePoint2>, S: PoleValue<FiniteReal>, K: KnotValue>(
    degree: u32,
    knots: K,
    poles: PolarNurbsPoles<P, S>,
    periodic: bool,
) -> Result<PolarPcurveNurbs, NurbsError> {
    require_curve_cardinality(degree, knots.knot_count(), poles.count(), "poles")?;
    if degree == 0 {
        return Err(NurbsError::Structure(
            "polar NURBS degree must be positive".to_owned(),
        ));
    }
    let poles = match poles {
        PolarNurbsPoles::Polynomial { poles } => PolarNurbsPoles::Polynomial {
            poles: poles
                .into_iter()
                .map(|pole| {
                    let (radial, axial) = map_polar_pole(pole.radial, pole.axial)?;
                    Ok(PolarNurbsPole { radial, axial })
                })
                .collect::<Result<Vec<_>, NurbsError>>()?,
        },
        PolarNurbsPoles::Rational { poles } => PolarNurbsPoles::Rational {
            poles: poles
                .into_iter()
                .map(|pole| {
                    let (radial, axial) = map_polar_pole(pole.radial, pole.axial)?;
                    Ok(WeightedPolarNurbsPole {
                        radial,
                        axial,
                        weight: pole.weight,
                    })
                })
                .collect::<Result<Vec<_>, NurbsError>>()?,
        },
    };
    let knots = admit_knots(knots, "")?;
    Ok(PolarPcurveNurbs {
        degree,
        knots,
        poles,
        periodic,
    })
}

#[cfg(test)]
mod tests;
