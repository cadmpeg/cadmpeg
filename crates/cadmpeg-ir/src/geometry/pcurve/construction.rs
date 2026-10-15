// SPDX-License-Identifier: Apache-2.0
//! Shared construction algorithms with an explicit refusal policy.

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

pub(super) fn pair_pcurve_lanes<P, W, A: NurbsAdmission>(
    admission: &A,
    points: Vec<P>,
    weights: Option<Vec<W>>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, A::Error>,
) -> Result<PcurveNurbsPoles<P>, A::Error> {
    let Some(weights) = weights else {
        return Ok(PcurveNurbsPoles::Polynomial { points });
    };
    require_weight_lane(admission, "pcurve poles", points.len(), weights.len())?;
    Ok(PcurveNurbsPoles::Rational {
        points: pair_weights(points, weights, |index, point, value| {
            Ok::<_, A::Error>(WeightedPole2 {
                point,
                weight: weight(index, value)?,
            })
        })?,
    })
}

pub(super) fn pair_polar_lanes<P, S, W, A: NurbsAdmission>(
    admission: &A,
    poles: Vec<PolarNurbsPole<P, S>>,
    weights: Option<Vec<W>>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, A::Error>,
) -> Result<PolarNurbsPoles<P, S>, A::Error> {
    let Some(weights) = weights else {
        return Ok(PolarNurbsPoles::Polynomial { poles });
    };
    require_weight_lane(admission, "polar poles", poles.len(), weights.len())?;
    Ok(PolarNurbsPoles::Rational {
        poles: pair_weights(poles, weights, |index, pole, value| {
            Ok::<_, A::Error>(WeightedPolarNurbsPole {
                radial: pole.radial,
                axial: pole.axial,
                weight: weight(index, value)?,
            })
        })?,
    })
}

fn map_pcurve_pole<P: PoleValue<FinitePoint2>, A: NurbsAdmission>(
    admission: &A,
    point: P,
) -> Result<FinitePoint2, A::Error> {
    match point.admit() {
        Some(point) => Ok(point),
        None => {
            Err(admission.structure(format_args!("control_points contains a non-finite point")))
        }
    }
}

fn map_pcurve_poles<P: PoleValue<FinitePoint2>, A: NurbsAdmission>(
    admission: &A,
    poles: PcurveNurbsPoles<P>,
) -> Result<PcurveNurbsPoles<FinitePoint2>, A::Error> {
    Ok(match poles {
        PcurveNurbsPoles::Polynomial { points } => PcurveNurbsPoles::Polynomial {
            points: points
                .into_iter()
                .map(|point| map_pcurve_pole(admission, point))
                .collect::<Result<Vec<_>, A::Error>>()?,
        },
        PcurveNurbsPoles::Rational { points } => PcurveNurbsPoles::Rational {
            points: points
                .into_iter()
                .map(|pole| {
                    Ok(WeightedPole2 {
                        point: map_pcurve_pole(admission, pole.point)?,
                        weight: pole.weight,
                    })
                })
                .collect::<Result<Vec<_>, A::Error>>()?,
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
        return Err(admission.structure(format_args!("pcurve NURBS degree must be positive")));
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
        None => Err(admission.structure(format_args!("poles contain a non-finite value"))),
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
        return Err(admission.structure(format_args!("polar NURBS degree must be positive")));
    }
    let poles = match poles {
        PolarNurbsPoles::Polynomial { poles } => PolarNurbsPoles::Polynomial {
            poles: poles
                .into_iter()
                .map(|pole| {
                    let (radial, axial) = map_polar_pole(admission, pole.radial, pole.axial)?;
                    Ok(PolarNurbsPole { radial, axial })
                })
                .collect::<Result<Vec<_>, A::Error>>()?,
        },
        PolarNurbsPoles::Rational { poles } => PolarNurbsPoles::Rational {
            poles: poles
                .into_iter()
                .map(|pole| {
                    let (radial, axial) = map_polar_pole(admission, pole.radial, pole.axial)?;
                    Ok(WeightedPolarNurbsPole {
                        radial,
                        axial,
                        weight: pole.weight,
                    })
                })
                .collect::<Result<Vec<_>, A::Error>>()?,
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
