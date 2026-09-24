// SPDX-License-Identifier: Apache-2.0
//! Parameter-space curves, NURBS payloads, and source parameterization.

use super::nurbs::{
    require_curve_cardinality, require_nondecreasing_knots, NurbsCurve, NurbsError, NurbsPoles3,
};
use super::{FitTolerance, MAX_GEOMETRY_NESTING};
use crate::ids::PcurveId;
use crate::math::{Point2, Point3};
use crate::scalar::{FiniteReal, NonZeroReal, PositiveReal};
use crate::transform::Transform2;
use crate::units::{FinitePoint2, FiniteVector, NonzeroPoint2};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One rational pole in parameter space: its position and its weight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WeightedPole2 {
    /// Pole position in parameter space.
    pub point: Point2,
    /// Rational weight at this pole.
    pub weight: NonZeroReal,
}

/// The poles of a parameter-space NURBS curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PcurveNurbsPoles {
    /// A polynomial pcurve: its poles carry no weight.
    Polynomial {
        /// Poles in parameter order.
        points: Vec<Point2>,
    },
    /// A rational pcurve: every pole carries its weight.
    Rational {
        /// Pole rows in parameter order.
        points: Vec<WeightedPole2>,
    },
}

impl PcurveNurbsPoles {
    /// Pair a source's pole lane with its weight lane.
    ///
    /// # Errors
    ///
    /// Refuses a weight lane that does not cover the poles, naming both counts,
    /// and a weight that is zero or non-finite, naming its index.
    pub fn from_lanes(points: Vec<Point2>, weights: Option<Vec<f64>>) -> Result<Self, NurbsError> {
        let Some(weights) = weights else {
            return Ok(Self::Polynomial { points });
        };
        if weights.len() != points.len() {
            return Err(NurbsError::WeightLaneLength {
                field: "pcurve poles".to_owned(),
                poles: points.len(),
                weights: weights.len(),
            });
        }
        Ok(Self::Rational {
            points: points
                .into_iter()
                .zip(weights)
                .enumerate()
                .map(|(index, (point, weight))| {
                    Ok(WeightedPole2 {
                        point,
                        weight: NonZeroReal::new(weight).ok_or(NurbsError::UnusableWeight {
                            field: "pcurve poles".to_owned(),
                            index,
                            weight,
                        })?,
                    })
                })
                .collect::<Result<Vec<_>, NurbsError>>()?,
        })
    }

    /// Count poles.
    #[must_use]
    pub fn count(&self) -> usize {
        match self {
            Self::Polynomial { points } => points.len(),
            Self::Rational { points } => points.len(),
        }
    }

    /// Pole positions in parameter order.
    #[must_use]
    pub fn points(&self) -> Vec<Point2> {
        match self {
            Self::Polynomial { points } => points.clone(),
            Self::Rational { points } => points.iter().map(|pole| pole.point).collect(),
        }
    }

    /// Rational weights in pole order, absent on a polynomial pcurve.
    #[must_use]
    pub fn weights(&self) -> Option<Vec<f64>> {
        match self {
            Self::Polynomial { .. } => None,
            Self::Rational { points } => {
                Some(points.iter().map(|pole| pole.weight.get()).collect())
            }
        }
    }

    /// Reverse the pole order.
    pub fn reverse(&mut self) {
        match self {
            Self::Polynomial { points } => points.reverse(),
            Self::Rational { points } => points.reverse(),
        }
    }

    /// Edit every pole position, keeping the prior positions on a refusal.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_points(
        &mut self,
        edit: impl FnMut(&mut Point2) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut candidate = self.clone();
        candidate.apply_points(edit)?;
        *self = candidate;
        Ok(())
    }

    /// Edit every pole position in place, keeping every accepted edit.
    ///
    /// A refusal leaves the lane partly edited, so the caller owns the copy
    /// that states the prior positions.
    fn apply_points(
        &mut self,
        mut edit: impl FnMut(&mut Point2) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        match self {
            Self::Polynomial { points } => {
                for point in points.iter_mut() {
                    edit(point)?;
                }
            }
            Self::Rational { points } => {
                for pole in points.iter_mut() {
                    edit(&mut pole.point)?;
                }
            }
        }
        Ok(())
    }
}

fn require_finite_points_2(field: &str, points: &[Point2]) -> Result<(), NurbsError> {
    if points.iter().all(Point2::is_finite) {
        Ok(())
    } else {
        Err(NurbsError::Structure(format!(
            "{field} contains a non-finite point"
        )))
    }
}

/// Parameter-space line with a finite origin and nonzero direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LinePcurveWire")]
pub struct LinePcurve {
    origin: FinitePoint2,
    direction: NonzeroPoint2,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LinePcurveWire {
    /// Parameter-space line origin.
    origin: Point2,
    /// Parameter-space line direction.
    direction: Point2,
}

impl LinePcurve {
    /// Unit-u line through the parameter-space origin.
    pub const U_AXIS: Self = Self {
        origin: FinitePoint2::ZERO,
        direction: NonzeroPoint2::U_AXIS,
    };

    /// Build a line from admitted parts. The argument types state the whole
    /// invariant, so nothing is checked again.
    #[must_use]
    pub const fn new(origin: FinitePoint2, direction: NonzeroPoint2) -> Self {
        Self { origin, direction }
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point2, direction: Point2) -> Result<Self, &'static str> {
        let origin = FinitePoint2::new(origin).ok_or("LinePcurve.origin must be finite")?;
        let direction = NonzeroPoint2::new(direction)
            .ok_or("LinePcurve.direction must be finite with squared norm greater than epsilon")?;
        Ok(Self { origin, direction })
    }

    /// Borrow the admitted origin. A caller that passes it on keeps the
    /// finiteness guarantee and performs no new admission.
    #[must_use]
    pub const fn origin(&self) -> &FinitePoint2 {
        &self.origin
    }

    /// Borrow the admitted direction. A caller that passes it on keeps the
    /// finite nonzero guarantee and performs no new admission.
    #[must_use]
    pub const fn direction(&self) -> &NonzeroPoint2 {
        &self.direction
    }
}

impl TryFrom<LinePcurveWire> for LinePcurve {
    type Error = &'static str;
    fn try_from(wire: LinePcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.origin, wire.direction)
    }
}

/// Polar harmonic curve with finite coefficients and nonzero radial variation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PolarHarmonicPcurveWire")]
pub struct PolarHarmonicPcurve {
    radial_center: FinitePoint2,
    radial_cos: FinitePoint2,
    radial_sin: FinitePoint2,
    axial_origin: FiniteReal,
    axial_cos: FiniteReal,
    axial_sin: FiniteReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PolarHarmonicPcurveWire {
    /// Constant radial coefficient.
    radial_center: Point2,
    /// Cosine radial coefficient.
    radial_cos: Point2,
    /// Sine radial coefficient.
    radial_sin: Point2,
    /// Constant axial coefficient.
    axial_origin: f64,
    /// Cosine axial coefficient.
    axial_cos: f64,
    /// Sine axial coefficient.
    axial_sin: f64,
}

impl PolarHarmonicPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        radial_center: Point2,
        radial_cos: Point2,
        radial_sin: Point2,
        axial_origin: f64,
        axial_cos: f64,
        axial_sin: f64,
    ) -> Result<Self, &'static str> {
        if !(radial_cos.u.hypot(radial_cos.v) > 0.0 || radial_sin.u.hypot(radial_sin.v) > 0.0) {
            return Err("PolarHarmonicPcurve.radial_cos/radial_sin must not both be zero");
        }
        let radial_center = FinitePoint2::new(radial_center)
            .ok_or("PolarHarmonicPcurve.radial_center must be finite")?;
        let radial_cos =
            FinitePoint2::new(radial_cos).ok_or("PolarHarmonicPcurve.radial_cos must be finite")?;
        let radial_sin =
            FinitePoint2::new(radial_sin).ok_or("PolarHarmonicPcurve.radial_sin must be finite")?;
        let axial_origin = FiniteReal::new(axial_origin)
            .ok_or("PolarHarmonicPcurve.axial_origin must be finite")?;
        let axial_cos =
            FiniteReal::new(axial_cos).ok_or("PolarHarmonicPcurve.axial_cos must be finite")?;
        let axial_sin =
            FiniteReal::new(axial_sin).ok_or("PolarHarmonicPcurve.axial_sin must be finite")?;
        Ok(Self {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin,
            axial_cos,
            axial_sin,
        })
    }

    /// Return the radial center.
    #[must_use]
    pub const fn radial_center(&self) -> &Point2 {
        self.radial_center.as_raw()
    }

    /// Return the radial cos.
    #[must_use]
    pub const fn radial_cos(&self) -> &Point2 {
        self.radial_cos.as_raw()
    }

    /// Return the radial sin.
    #[must_use]
    pub const fn radial_sin(&self) -> &Point2 {
        self.radial_sin.as_raw()
    }

    /// Return the axial origin.
    #[must_use]
    pub const fn axial_origin(&self) -> f64 {
        self.axial_origin.get()
    }

    /// Return the axial cos.
    #[must_use]
    pub const fn axial_cos(&self) -> f64 {
        self.axial_cos.get()
    }

    /// Return the axial sin.
    #[must_use]
    pub const fn axial_sin(&self) -> f64 {
        self.axial_sin.get()
    }
}

impl TryFrom<PolarHarmonicPcurveWire> for PolarHarmonicPcurve {
    type Error = &'static str;
    fn try_from(wire: PolarHarmonicPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.radial_center,
            wire.radial_cos,
            wire.radial_sin,
            wire.axial_origin,
            wire.axial_cos,
            wire.axial_sin,
        )
    }
}

/// Spherical great-circle chart with finite coefficients and nonzero azimuth rate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SphericalGreatCirclePcurveWire")]
pub struct SphericalGreatCirclePcurve {
    azimuth_origin: FiniteReal,
    azimuth_rate: FiniteReal,
    plane_phase: FiniteReal,
    plane_slope: FiniteReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SphericalGreatCirclePcurveWire {
    /// Azimuth at the parameter origin.
    azimuth_origin: f64,
    /// Azimuth change for one unit of parameter.
    azimuth_rate: f64,
    /// Phase of the great-circle plane.
    plane_phase: f64,
    /// Slope of the great-circle plane.
    plane_slope: f64,
}

impl SphericalGreatCirclePcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        azimuth_origin: f64,
        azimuth_rate: f64,
        plane_phase: f64,
        plane_slope: f64,
    ) -> Result<Self, &'static str> {
        if azimuth_rate == 0.0 {
            return Err("SphericalGreatCirclePcurve.azimuth_rate must be nonzero");
        }
        let azimuth_origin = FiniteReal::new(azimuth_origin)
            .ok_or("SphericalGreatCirclePcurve.azimuth_origin must be finite")?;
        let azimuth_rate = FiniteReal::new(azimuth_rate)
            .ok_or("SphericalGreatCirclePcurve.azimuth_rate must be finite")?;
        let plane_phase = FiniteReal::new(plane_phase)
            .ok_or("SphericalGreatCirclePcurve.plane_phase must be finite")?;
        let plane_slope = FiniteReal::new(plane_slope)
            .ok_or("SphericalGreatCirclePcurve.plane_slope must be finite")?;
        Ok(Self {
            azimuth_origin,
            azimuth_rate,
            plane_phase,
            plane_slope,
        })
    }

    /// Return the azimuth origin.
    #[must_use]
    pub const fn azimuth_origin(&self) -> f64 {
        self.azimuth_origin.get()
    }

    /// Return the azimuth rate.
    #[must_use]
    pub const fn azimuth_rate(&self) -> f64 {
        self.azimuth_rate.get()
    }

    /// Return the plane phase.
    #[must_use]
    pub const fn plane_phase(&self) -> f64 {
        self.plane_phase.get()
    }

    /// Return the plane slope.
    #[must_use]
    pub const fn plane_slope(&self) -> f64 {
        self.plane_slope.get()
    }

    /// The same great circle traversed as `reflection - t`. The azimuth
    /// origin moves to `azimuth_origin + azimuth_rate * reflection` and the
    /// rate changes sign. The negated rate stays finite and nonzero and the
    /// plane is kept, so only the moved origin is checked. The result is
    /// absent when the moved origin is not finite.
    #[must_use]
    pub fn reflected(&self, reflection: f64) -> Option<Self> {
        let azimuth_origin =
            FiniteReal::new(self.azimuth_origin.get() + self.azimuth_rate.get() * reflection)?;
        Some(Self {
            azimuth_origin,
            azimuth_rate: self.azimuth_rate.negated(),
            plane_phase: self.plane_phase,
            plane_slope: self.plane_slope,
        })
    }
}

impl TryFrom<SphericalGreatCirclePcurveWire> for SphericalGreatCirclePcurve {
    type Error = &'static str;
    fn try_from(wire: SphericalGreatCirclePcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.azimuth_origin,
            wire.azimuth_rate,
            wire.plane_phase,
            wire.plane_slope,
        )
    }
}

/// Parameter-space circle with a positive radius and finite nonzero axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CirclePcurveWire")]
pub struct CirclePcurve {
    center: FinitePoint2,
    x_axis: FinitePoint2,
    y_axis: FinitePoint2,
    radius: PositiveReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct CirclePcurveWire {
    /// Parameter-space circle center.
    center: Point2,
    /// First parameter-space axis.
    x_axis: Point2,
    /// Second parameter-space axis.
    y_axis: Point2,
    /// Circle radius in parameter space.
    radius: f64,
}

impl CirclePcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point2,
        x_axis: Point2,
        y_axis: Point2,
        radius: f64,
    ) -> Result<Self, &'static str> {
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("CirclePcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("CirclePcurve.y_axis must be nonzero");
        }
        let center = FinitePoint2::new(center).ok_or("CirclePcurve.center must be finite")?;
        let x_axis = FinitePoint2::new(x_axis).ok_or("CirclePcurve.x_axis must be finite")?;
        let y_axis = FinitePoint2::new(y_axis).ok_or("CirclePcurve.y_axis must be finite")?;
        let radius =
            PositiveReal::new(radius).ok_or("CirclePcurve.radius must be positive and finite")?;
        Ok(Self {
            center,
            x_axis,
            y_axis,
            radius,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the x axis.
    #[must_use]
    pub const fn x_axis(&self) -> &Point2 {
        self.x_axis.as_raw()
    }

    /// Return the y axis.
    #[must_use]
    pub const fn y_axis(&self) -> &Point2 {
        self.y_axis.as_raw()
    }

    /// Return the radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
    }
}

impl TryFrom<CirclePcurveWire> for CirclePcurve {
    type Error = &'static str;
    fn try_from(wire: CirclePcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.x_axis, wire.y_axis, wire.radius)
    }
}

/// Parameter-space ellipse with positive radii and finite nonzero axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "EllipsePcurveWire")]
pub struct EllipsePcurve {
    center: FinitePoint2,
    x_axis: FinitePoint2,
    y_axis: FinitePoint2,
    major_radius: PositiveReal,
    minor_radius: PositiveReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct EllipsePcurveWire {
    /// Parameter-space ellipse center.
    center: Point2,
    /// First parameter-space axis.
    x_axis: Point2,
    /// Second parameter-space axis.
    y_axis: Point2,
    /// Major semiaxis radius in parameter space.
    major_radius: f64,
    /// Minor semiaxis radius in parameter space.
    minor_radius: f64,
}

impl EllipsePcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point2,
        x_axis: Point2,
        y_axis: Point2,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("EllipsePcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("EllipsePcurve.y_axis must be nonzero");
        }
        let center = FinitePoint2::new(center).ok_or("EllipsePcurve.center must be finite")?;
        let x_axis = FinitePoint2::new(x_axis).ok_or("EllipsePcurve.x_axis must be finite")?;
        let y_axis = FinitePoint2::new(y_axis).ok_or("EllipsePcurve.y_axis must be finite")?;
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("EllipsePcurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveReal::new(minor_radius)
            .ok_or("EllipsePcurve.minor_radius must be positive and finite")?;
        Ok(Self {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the x axis.
    #[must_use]
    pub const fn x_axis(&self) -> &Point2 {
        self.x_axis.as_raw()
    }

    /// Return the y axis.
    #[must_use]
    pub const fn y_axis(&self) -> &Point2 {
        self.y_axis.as_raw()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
    }

    /// The same ellipse traversed as `-t`: the second axis changes sign.
    /// Negation keeps the axis finite and nonzero and every other field is
    /// kept, so nothing is checked.
    #[must_use]
    pub fn reversed_about_zero(&self) -> Self {
        Self {
            y_axis: self.y_axis.negated(),
            ..*self
        }
    }
}

impl TryFrom<EllipsePcurveWire> for EllipsePcurve {
    type Error = &'static str;
    fn try_from(wire: EllipsePcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.x_axis,
            wire.y_axis,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Harmonic parameter-space curve with finite coefficients and nonzero variation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HarmonicPcurveWire")]
pub struct HarmonicPcurve {
    center: FinitePoint2,
    cosine: FinitePoint2,
    sine: FinitePoint2,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HarmonicPcurveWire {
    /// Constant parameter-space coefficient.
    center: Point2,
    /// Cosine parameter-space coefficient.
    cosine: Point2,
    /// Sine parameter-space coefficient.
    sine: Point2,
}

impl HarmonicPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(center: Point2, cosine: Point2, sine: Point2) -> Result<Self, &'static str> {
        if !(cosine.u.hypot(cosine.v) > 0.0 || sine.u.hypot(sine.v) > 0.0) {
            return Err("HarmonicPcurve.cosine/sine must not both be zero");
        }
        let center = FinitePoint2::new(center).ok_or("HarmonicPcurve.center must be finite")?;
        let cosine = FinitePoint2::new(cosine).ok_or("HarmonicPcurve.cosine must be finite")?;
        let sine = FinitePoint2::new(sine).ok_or("HarmonicPcurve.sine must be finite")?;
        Ok(Self {
            center,
            cosine,
            sine,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the cosine.
    #[must_use]
    pub const fn cosine(&self) -> &Point2 {
        self.cosine.as_raw()
    }

    /// Return the sine.
    #[must_use]
    pub const fn sine(&self) -> &Point2 {
        self.sine.as_raw()
    }
}

impl TryFrom<HarmonicPcurveWire> for HarmonicPcurve {
    type Error = &'static str;
    fn try_from(wire: HarmonicPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.cosine, wire.sine)
    }
}

/// Parameter-space parabola with a positive focal distance and finite nonzero axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ParabolaPcurveWire")]
pub struct ParabolaPcurve {
    vertex: FinitePoint2,
    x_axis: FinitePoint2,
    y_axis: FinitePoint2,
    focal_distance: PositiveReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct ParabolaPcurveWire {
    /// Parameter-space parabola vertex.
    vertex: Point2,
    /// First parameter-space axis.
    x_axis: Point2,
    /// Second parameter-space axis.
    y_axis: Point2,
    /// Distance from the vertex to the focus, in parameter space.
    focal_distance: f64,
}

impl ParabolaPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        vertex: Point2,
        x_axis: Point2,
        y_axis: Point2,
        focal_distance: f64,
    ) -> Result<Self, &'static str> {
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("ParabolaPcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("ParabolaPcurve.y_axis must be nonzero");
        }
        let vertex = FinitePoint2::new(vertex).ok_or("ParabolaPcurve.vertex must be finite")?;
        let x_axis = FinitePoint2::new(x_axis).ok_or("ParabolaPcurve.x_axis must be finite")?;
        let y_axis = FinitePoint2::new(y_axis).ok_or("ParabolaPcurve.y_axis must be finite")?;
        let focal_distance = PositiveReal::new(focal_distance)
            .ok_or("ParabolaPcurve.focal_distance must be positive and finite")?;
        Ok(Self {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        })
    }

    /// Return the vertex.
    #[must_use]
    pub const fn vertex(&self) -> &Point2 {
        self.vertex.as_raw()
    }

    /// Return the x axis.
    #[must_use]
    pub const fn x_axis(&self) -> &Point2 {
        self.x_axis.as_raw()
    }

    /// Return the y axis.
    #[must_use]
    pub const fn y_axis(&self) -> &Point2 {
        self.y_axis.as_raw()
    }

    /// Return the focal distance.
    #[must_use]
    pub const fn focal_distance(&self) -> f64 {
        self.focal_distance.get()
    }

    /// The same parabola traversed as `-t`: the second axis changes sign.
    /// Negation keeps the axis finite and nonzero and every other field is
    /// kept, so nothing is checked.
    #[must_use]
    pub fn reversed_about_zero(&self) -> Self {
        Self {
            y_axis: self.y_axis.negated(),
            ..*self
        }
    }
}

impl TryFrom<ParabolaPcurveWire> for ParabolaPcurve {
    type Error = &'static str;
    fn try_from(wire: ParabolaPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.vertex, wire.x_axis, wire.y_axis, wire.focal_distance)
    }
}

/// Parameter-space hyperbola with positive radii and finite nonzero axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HyperbolaPcurveWire")]
pub struct HyperbolaPcurve {
    center: FinitePoint2,
    x_axis: FinitePoint2,
    y_axis: FinitePoint2,
    major_radius: PositiveReal,
    minor_radius: PositiveReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HyperbolaPcurveWire {
    /// Parameter-space hyperbola center.
    center: Point2,
    /// First parameter-space axis.
    x_axis: Point2,
    /// Second parameter-space axis.
    y_axis: Point2,
    /// Transverse semiaxis radius in parameter space.
    major_radius: f64,
    /// Conjugate semiaxis radius in parameter space.
    minor_radius: f64,
}

impl HyperbolaPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point2,
        x_axis: Point2,
        y_axis: Point2,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("HyperbolaPcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("HyperbolaPcurve.y_axis must be nonzero");
        }
        let center = FinitePoint2::new(center).ok_or("HyperbolaPcurve.center must be finite")?;
        let x_axis = FinitePoint2::new(x_axis).ok_or("HyperbolaPcurve.x_axis must be finite")?;
        let y_axis = FinitePoint2::new(y_axis).ok_or("HyperbolaPcurve.y_axis must be finite")?;
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("HyperbolaPcurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveReal::new(minor_radius)
            .ok_or("HyperbolaPcurve.minor_radius must be positive and finite")?;
        Ok(Self {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the x axis.
    #[must_use]
    pub const fn x_axis(&self) -> &Point2 {
        self.x_axis.as_raw()
    }

    /// Return the y axis.
    #[must_use]
    pub const fn y_axis(&self) -> &Point2 {
        self.y_axis.as_raw()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
    }

    /// The same hyperbola traversed as `-t`: the second axis changes sign.
    /// Negation keeps the axis finite and nonzero and every other field is
    /// kept, so nothing is checked.
    #[must_use]
    pub fn reversed_about_zero(&self) -> Self {
        Self {
            y_axis: self.y_axis.negated(),
            ..*self
        }
    }
}

impl TryFrom<HyperbolaPcurveWire> for HyperbolaPcurve {
    type Error = &'static str;
    fn try_from(wire: HyperbolaPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.x_axis,
            wire.y_axis,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Hyperbolic parameter-space curve with finite coefficients and nonzero variation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HyperbolicPcurveWire")]
pub struct HyperbolicPcurve {
    center: FinitePoint2,
    cosine: FinitePoint2,
    sine: FinitePoint2,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HyperbolicPcurveWire {
    /// Constant parameter-space coefficient.
    center: Point2,
    /// Hyperbolic-cosine parameter-space coefficient.
    cosine: Point2,
    /// Hyperbolic-sine parameter-space coefficient.
    sine: Point2,
}

impl HyperbolicPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(center: Point2, cosine: Point2, sine: Point2) -> Result<Self, &'static str> {
        if !(cosine.u.hypot(cosine.v) > 0.0 || sine.u.hypot(sine.v) > 0.0) {
            return Err("HyperbolicPcurve.cosine/sine must not both be zero");
        }
        let center = FinitePoint2::new(center).ok_or("HyperbolicPcurve.center must be finite")?;
        let cosine = FinitePoint2::new(cosine).ok_or("HyperbolicPcurve.cosine must be finite")?;
        let sine = FinitePoint2::new(sine).ok_or("HyperbolicPcurve.sine must be finite")?;
        Ok(Self {
            center,
            cosine,
            sine,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the cosine.
    #[must_use]
    pub const fn cosine(&self) -> &Point2 {
        self.cosine.as_raw()
    }

    /// Return the sine.
    #[must_use]
    pub const fn sine(&self) -> &Point2 {
        self.sine.as_raw()
    }
}

impl TryFrom<HyperbolicPcurveWire> for HyperbolicPcurve {
    type Error = &'static str;
    fn try_from(wire: HyperbolicPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.cosine, wire.sine)
    }
}

/// Parameter-space trim with a finite ordered interval.
///
/// `parameter_range` is stated in the basis's own parameter space and is the
/// carrier's declared domain: the trimmed pcurve exists on that interval and a
/// coedge use range over it is checked against it. Equal endpoints state no
/// interval, and the basis's parameterization governs instead.
///
/// The trim reparameterizes nothing, so
/// [`pcurve_uv`](crate::eval::pcurve_uv) hands `t` to the basis unchanged. It
/// does so at every `t`, inside the interval and outside it, because pcurve
/// evaluation is total: it extrapolates past any declared domain, a NURBS
/// carrier's knot interval included. A declared domain and an evaluable
/// parameter are two different questions, and this carrier answers only the
/// first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TrimmedPcurveWire")]
pub struct TrimmedPcurve {
    parameter_range: [f64; 2],
    #[serde(default = "crate::default_true")]
    same_sense: bool,
    basis: Box<PcurveGeometry>,
    #[serde(skip)]
    depth: usize,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TrimmedPcurveWire {
    /// Native parameter interval retained from the basis.
    parameter_range: [f64; 2],
    /// Whether the trim follows increasing basis parameters.
    #[serde(default = "crate::default_true")]
    same_sense: bool,
    /// Curve this curve trims.
    basis: Box<PcurveGeometry>,
}

impl TrimmedPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        parameter_range: [f64; 2],
        same_sense: bool,
        basis: Box<PcurveGeometry>,
    ) -> Result<Self, &'static str> {
        if !(parameter_range.iter().all(|v| v.is_finite())
            && parameter_range[0] <= parameter_range[1])
        {
            return Err("TrimmedPcurve.parameter_range must be finite and ordered");
        }
        let depth = nesting_depth_over(&basis)
            .ok_or("TrimmedPcurve.basis nests past the admitted inline basis depth")?;
        Ok(Self {
            parameter_range,
            same_sense,
            basis,
            depth,
        })
    }

    /// Return the parameter range.
    #[must_use]
    pub const fn parameter_range(&self) -> &[f64; 2] {
        &self.parameter_range
    }

    /// Return the same sense.
    #[must_use]
    pub const fn same_sense(&self) -> bool {
        self.same_sense
    }

    /// Return the basis.
    #[must_use]
    pub const fn basis(&self) -> &PcurveGeometry {
        &self.basis
    }
}

impl TryFrom<TrimmedPcurveWire> for TrimmedPcurve {
    type Error = &'static str;
    fn try_from(wire: TrimmedPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.parameter_range, wire.same_sense, wire.basis)
    }
}

/// Parameter-space offset with a finite signed distance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "OffsetPcurveWire")]
pub struct OffsetPcurve {
    distance: FiniteReal,
    basis: Box<PcurveGeometry>,
    #[serde(skip)]
    depth: usize,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct OffsetPcurveWire {
    /// Signed parameter-space offset distance.
    distance: f64,
    /// Curve this curve is offset from.
    basis: Box<PcurveGeometry>,
}

impl OffsetPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(distance: f64, basis: Box<PcurveGeometry>) -> Result<Self, &'static str> {
        let distance = FiniteReal::new(distance).ok_or("OffsetPcurve.distance must be finite")?;
        let depth = nesting_depth_over(&basis)
            .ok_or("OffsetPcurve.basis nests past the admitted inline basis depth")?;
        Ok(Self {
            distance,
            basis,
            depth,
        })
    }

    /// Return the distance.
    #[must_use]
    pub const fn distance(&self) -> f64 {
        self.distance.get()
    }

    /// Return the basis.
    #[must_use]
    pub const fn basis(&self) -> &PcurveGeometry {
        &self.basis
    }
}

impl TryFrom<OffsetPcurveWire> for OffsetPcurve {
    type Error = &'static str;
    fn try_from(wire: OffsetPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.distance, wire.basis)
    }
}

/// The shape of a parameter-space (u, v) curve on a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PcurveGeometry {
    /// A straight line in parameter space.
    Line(LinePcurve),
    /// Polar angle and axial coordinate of a first-order harmonic spatial curve.
    PolarHarmonic(PolarHarmonicPcurve),
    /// Polar angle and axial coordinate obtained from a rational NURBS vector.
    PolarNurbs {
        /// Checked polar NURBS payload.
        #[serde(flatten)]
        #[cfg_attr(feature = "schema", schemars(flatten, with = "PolarPcurveNurbsWire"))]
        nurbs: PolarPcurveNurbs,
    },
    /// Great-circle locus in a sphere's azimuth/latitude parameter chart.
    SphericalGreatCircle(SphericalGreatCirclePcurve),
    /// Full circle in parameter space.
    Circle(CirclePcurve),
    /// Full ellipse in parameter space.
    Ellipse(EllipsePcurve),
    /// General first-order harmonic curve in parameter space.
    Harmonic(HarmonicPcurve),
    /// Parabola in parameter space.
    Parabola(ParabolaPcurve),
    /// Hyperbola in parameter space.
    Hyperbola(HyperbolaPcurve),
    /// General first-order hyperbolic curve in parameter space.
    Hyperbolic(HyperbolicPcurve),
    /// A free-form NURBS curve in parameter space (control points are (u, v)).
    Nurbs {
        /// Checked parameter-space NURBS payload.
        #[serde(flatten)]
        #[cfg_attr(feature = "schema", schemars(flatten))]
        nurbs: PcurveNurbs,
    },
    /// Affine replica of a parent pcurve in the same parameter space.
    Transformed(PlacedPcurve),
    /// Parameter restriction of an exact basis pcurve.
    Trimmed(TrimmedPcurve),
    /// Signed planar offset of an exact basis pcurve.
    Offset(OffsetPcurve),
}

/// One paired radial and axial pole of a polar parameter-space NURBS.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PolarNurbsPole {
    /// Euclidean radial-plane pole.
    pub radial: Point2,
    /// Axial pole value.
    pub axial: f64,
}

/// Checked polar parameter-space NURBS payload.
#[derive(Debug, Clone, PartialEq)]
pub struct PolarPcurveNurbs {
    degree: u32,
    knots: Vec<f64>,
    poles: PolarNurbsPoles,
    periodic: bool,
}

/// One rational polar pole: its radial and axial halves and its weight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WeightedPolarNurbsPole {
    /// Euclidean radial-plane pole.
    pub radial: Point2,
    /// Axial pole value.
    pub axial: f64,
    /// Rational weight at this pole.
    pub weight: NonZeroReal,
}

/// The poles of a polar parameter-space NURBS curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PolarNurbsPoles {
    /// A polynomial polar curve: its poles carry no weight.
    Polynomial {
        /// Poles in parameter order.
        poles: Vec<PolarNurbsPole>,
    },
    /// A rational polar curve: every pole carries its weight.
    Rational {
        /// Pole rows in parameter order.
        poles: Vec<WeightedPolarNurbsPole>,
    },
}

impl PolarNurbsPoles {
    /// Pair a source's pole lane with its weight lane.
    ///
    /// # Errors
    ///
    /// Refuses a weight lane that does not cover the poles, naming both counts,
    /// and a weight that is zero or non-finite, naming its index.
    pub fn from_lanes(
        poles: Vec<PolarNurbsPole>,
        weights: Option<Vec<f64>>,
    ) -> Result<Self, NurbsError> {
        let Some(weights) = weights else {
            return Ok(Self::Polynomial { poles });
        };
        if weights.len() != poles.len() {
            return Err(NurbsError::WeightLaneLength {
                field: "polar poles".to_owned(),
                poles: poles.len(),
                weights: weights.len(),
            });
        }
        Ok(Self::Rational {
            poles: poles
                .into_iter()
                .zip(weights)
                .enumerate()
                .map(|(index, (pole, weight))| {
                    Ok(WeightedPolarNurbsPole {
                        radial: pole.radial,
                        axial: pole.axial,
                        weight: NonZeroReal::new(weight).ok_or(NurbsError::UnusableWeight {
                            field: "polar poles".to_owned(),
                            index,
                            weight,
                        })?,
                    })
                })
                .collect::<Result<Vec<_>, NurbsError>>()?,
        })
    }

    /// Count poles.
    #[must_use]
    pub fn count(&self) -> usize {
        match self {
            Self::Polynomial { poles } => poles.len(),
            Self::Rational { poles } => poles.len(),
        }
    }

    /// Paired radial and axial poles in parameter order.
    #[must_use]
    pub fn poles(&self) -> Vec<PolarNurbsPole> {
        match self {
            Self::Polynomial { poles } => poles.clone(),
            Self::Rational { poles } => poles
                .iter()
                .map(|pole| PolarNurbsPole {
                    radial: pole.radial,
                    axial: pole.axial,
                })
                .collect(),
        }
    }

    /// Rational weights in pole order, absent on a polynomial curve.
    #[must_use]
    pub fn weights(&self) -> Option<Vec<f64>> {
        match self {
            Self::Polynomial { .. } => None,
            Self::Rational { poles } => Some(poles.iter().map(|pole| pole.weight.get()).collect()),
        }
    }
}

impl PolarPcurveNurbs {
    /// Build a polar NURBS from its pole rows.
    ///
    /// A pole is one row carrying its radial and axial halves together, so
    /// there are no two lists to pair up and no length to compare.
    pub fn new(
        degree: u32,
        knots: Vec<f64>,
        poles: PolarNurbsPoles,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        require_curve_cardinality(degree, knots.len(), poles.count(), "poles")?;
        if degree == 0 {
            return Err(NurbsError::Structure(
                "polar NURBS degree must be positive".into(),
            ));
        }
        if !poles
            .poles()
            .iter()
            .all(|pole| pole.radial.is_finite() && pole.axial.is_finite())
        {
            return Err(NurbsError::Structure(
                "poles contain a non-finite value".into(),
            ));
        }
        require_nondecreasing_knots(&knots)?;
        Ok(Self {
            degree,
            knots,
            poles,
            periodic,
        })
    }

    /// Polynomial degree shared by every component.
    pub const fn degree(&self) -> u32 {
        self.degree
    }

    /// Expanded knot vector.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// Build a polar NURBS from a source's pole and weight lanes.
    pub fn from_lanes(
        degree: u32,
        knots: Vec<f64>,
        poles: Vec<PolarNurbsPole>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        let poles = PolarNurbsPoles::from_lanes(poles, weights)?;
        Self::new(degree, knots, poles, periodic)
    }

    /// Paired radial and axial poles.
    pub fn poles(&self) -> Vec<PolarNurbsPole> {
        self.poles.poles()
    }

    /// Rational weights in pole order.
    pub fn weights(&self) -> Option<Vec<f64>> {
        self.poles.weights()
    }

    /// Whether the NURBS parameterization is periodic.
    pub const fn periodic(&self) -> bool {
        self.periodic
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PolarPcurveNurbsWire {
    degree: u32,
    knots: Vec<f64>,
    poles: PolarNurbsPoles,
    #[serde(default)]
    periodic: bool,
}

impl Serialize for PolarPcurveNurbs {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PolarPcurveNurbsWire {
            degree: self.degree,
            knots: self.knots.clone(),
            poles: self.poles.clone(),
            periodic: self.periodic,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PolarPcurveNurbs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PolarPcurveNurbsWire::deserialize(deserializer)?;
        Self::new(wire.degree, wire.knots, wire.poles, wire.periodic)
            .map_err(serde::de::Error::custom)
    }
}

/// Checked two-dimensional NURBS payload for pcurves and sketch curves.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PcurveNurbs {
    degree: u32,
    knots: Vec<f64>,
    poles: PcurveNurbsPoles,
    #[serde(default)]
    periodic: bool,
}

impl PcurveNurbs {
    /// Build a parameter-space NURBS with consistent cardinalities.
    pub fn new(
        degree: u32,
        knots: Vec<f64>,
        poles: PcurveNurbsPoles,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        require_curve_cardinality(degree, knots.len(), poles.count(), "control_points")?;
        if degree == 0 {
            return Err(NurbsError::Structure(
                "pcurve NURBS degree must be positive".into(),
            ));
        }
        require_finite_points_2("control_points", &poles.points())?;
        require_nondecreasing_knots(&knots)?;
        Ok(Self {
            degree,
            knots,
            poles,
            periodic,
        })
    }

    /// Lift each two-dimensional pole into model space, keeping its weight.
    pub fn lift(&self, lift: impl FnMut(Point2) -> Point3) -> Result<NurbsCurve, NurbsError> {
        let points: Vec<Point3> = self.poles.points().into_iter().map(lift).collect();
        let poles = NurbsPoles3::from_lanes(points, self.poles.weights())?;
        NurbsCurve::new(self.degree, self.knots.clone(), poles, self.periodic)
    }

    /// Curve degree.
    pub const fn degree(&self) -> u32 {
        self.degree
    }

    /// Full knot vector.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// Build a parameter-space NURBS from a source's pole and weight lanes.
    pub fn from_lanes(
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<Point2>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        let poles = PcurveNurbsPoles::from_lanes(control_points, weights)?;
        Self::new(degree, knots, poles, periodic)
    }

    /// Poles in parameter order, with the pcurve's rational form.
    pub const fn pole_rows(&self) -> &PcurveNurbsPoles {
        &self.poles
    }

    /// Control points in parameter order.
    pub fn control_points(&self) -> Vec<Point2> {
        self.poles.points()
    }

    /// Atomically edit pole positions and preserve finite coordinates.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnMut(&mut Point2) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut poles = self.poles.clone();
        poles.apply_points(edit)?;
        require_finite_points_2("control_points", &poles.points())?;
        self.poles = poles;
        Ok(())
    }

    /// Rational weights in pole order.
    pub fn weights(&self) -> Option<Vec<f64>> {
        self.poles.weights()
    }

    /// Whether the parameter-space curve is periodic.
    pub const fn periodic(&self) -> bool {
        self.periodic
    }

    /// Reverse poles, weights, and the signed knot parameterization together.
    pub fn reverse_parameterization(&mut self) {
        self.poles.reverse();
        self.knots.reverse();
        for knot in &mut self.knots {
            *knot = -*knot;
        }
    }
}

impl<'de> Deserialize<'de> for PcurveNurbs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            degree: u32,
            knots: Vec<f64>,
            poles: PcurveNurbsPoles,
            #[serde(default)]
            periodic: bool,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.degree, wire.knots, wire.poles, wire.periodic)
            .map_err(serde::de::Error::custom)
    }
}

/// The depth a new nesting carrier over `basis` would hold, or `None` when
/// that is past [`MAX_GEOMETRY_NESTING`].
///
/// A pcurve nests through three wrappers rather than one: [`PlacedPcurve`],
/// [`TrimmedPcurve`] and [`OffsetPcurve`] each hold one inline basis, and the
/// admitted depth counts all three together. Each stores its own depth, so
/// this reads one field.
fn nesting_depth_over(basis: &PcurveGeometry) -> Option<usize> {
    basis
        .nesting_depth()
        .checked_add(1)
        .filter(|depth| *depth <= MAX_GEOMETRY_NESTING)
}

/// Affine replica of a parent pcurve in the same parameter space.
///
/// `try_new` is the only constructor and refuses a chain deeper than
/// [`MAX_GEOMETRY_NESTING`], so no [`PcurveGeometry`] value nests past the
/// bound however it was built or read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PlacedPcurveWire")]
pub struct PlacedPcurve {
    basis: Box<PcurveGeometry>,
    transform: Transform2,
    #[serde(skip)]
    depth: usize,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PlacedPcurveWire {
    /// Exact parent pcurve and its parameterization.
    basis: Box<PcurveGeometry>,
    /// Two-dimensional affine map from parent coordinates to replica coordinates.
    transform: Transform2,
}

impl PlacedPcurve {
    /// Place a basis pcurve, refusing a chain past [`MAX_GEOMETRY_NESTING`].
    ///
    /// # Errors
    ///
    /// Refuses a basis already at the bound, whose placement would produce a
    /// carrier one deeper than the IR admits.
    pub fn try_new(
        basis: Box<PcurveGeometry>,
        transform: Transform2,
    ) -> Result<Self, &'static str> {
        let depth = nesting_depth_over(&basis)
            .ok_or("PlacedPcurve.basis nests past the admitted inline basis depth")?;
        Ok(Self {
            basis,
            transform,
            depth,
        })
    }

    /// Return the basis.
    #[must_use]
    pub const fn basis(&self) -> &PcurveGeometry {
        &self.basis
    }

    /// Return the transform.
    #[must_use]
    pub const fn transform(&self) -> &Transform2 {
        &self.transform
    }
}

impl TryFrom<PlacedPcurveWire> for PlacedPcurve {
    type Error = &'static str;
    fn try_from(wire: PlacedPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.basis, wire.transform)
    }
}

impl PcurveGeometry {
    /// Nesting carriers enclosing the leaf of this carrier's inline chain.
    #[must_use]
    pub(crate) const fn nesting_depth(&self) -> usize {
        match self {
            Self::Transformed(placed) => placed.depth,
            Self::Trimmed(trimmed) => trimmed.depth,
            Self::Offset(offset) => offset.depth,
            Self::Line(_)
            | Self::PolarHarmonic(_)
            | Self::PolarNurbs { .. }
            | Self::SphericalGreatCircle(_)
            | Self::Circle(_)
            | Self::Ellipse(_)
            | Self::Harmonic(_)
            | Self::Parabola(_)
            | Self::Hyperbola(_)
            | Self::Hyperbolic(_)
            | Self::Nurbs { .. } => 0,
        }
    }

    /// Scale chart coordinates atomically without changing the curve parameterization.
    pub fn try_scale_coordinates(&mut self, scales: [f64; 2]) -> Result<(), String> {
        let [u_scale, v_scale] = scales;
        let scale = |point: Point2| Point2::new(point.u * u_scale, point.v * v_scale);
        let isotropic = u_scale == v_scale;
        let scaled = match self {
            Self::Line(line) => Self::Line(LinePcurve::try_new(
                scale(*line.origin().as_raw()),
                scale(*line.direction().as_raw()),
            )?),
            Self::Circle(circle) if isotropic => Self::Circle(CirclePcurve::try_new(
                scale(*circle.center()),
                *circle.x_axis(),
                *circle.y_axis(),
                circle.radius() * u_scale,
            )?),
            Self::Circle(circle) => Self::Harmonic(HarmonicPcurve::try_new(
                scale(*circle.center()),
                scale(Point2::new(
                    circle.radius() * circle.x_axis().u,
                    circle.radius() * circle.x_axis().v,
                )),
                scale(Point2::new(
                    circle.radius() * circle.y_axis().u,
                    circle.radius() * circle.y_axis().v,
                )),
            )?),
            Self::Ellipse(ellipse) if isotropic => Self::Ellipse(EllipsePcurve::try_new(
                scale(*ellipse.center()),
                *ellipse.x_axis(),
                *ellipse.y_axis(),
                ellipse.major_radius() * u_scale,
                ellipse.minor_radius() * u_scale,
            )?),
            Self::Ellipse(ellipse) => Self::Harmonic(HarmonicPcurve::try_new(
                scale(*ellipse.center()),
                scale(Point2::new(
                    ellipse.major_radius() * ellipse.x_axis().u,
                    ellipse.major_radius() * ellipse.x_axis().v,
                )),
                scale(Point2::new(
                    ellipse.minor_radius() * ellipse.y_axis().u,
                    ellipse.minor_radius() * ellipse.y_axis().v,
                )),
            )?),
            Self::Parabola(parabola) => Self::Parabola(ParabolaPcurve::try_new(
                scale(*parabola.vertex()),
                scale(*parabola.x_axis()),
                scale(*parabola.y_axis()),
                parabola.focal_distance(),
            )?),
            Self::Hyperbola(hyperbola) if isotropic => Self::Hyperbola(HyperbolaPcurve::try_new(
                scale(*hyperbola.center()),
                *hyperbola.x_axis(),
                *hyperbola.y_axis(),
                hyperbola.major_radius() * u_scale,
                hyperbola.minor_radius() * u_scale,
            )?),
            Self::Hyperbola(hyperbola) => Self::Hyperbolic(HyperbolicPcurve::try_new(
                scale(*hyperbola.center()),
                scale(Point2::new(
                    hyperbola.major_radius() * hyperbola.x_axis().u,
                    hyperbola.major_radius() * hyperbola.x_axis().v,
                )),
                scale(Point2::new(
                    hyperbola.minor_radius() * hyperbola.y_axis().u,
                    hyperbola.minor_radius() * hyperbola.y_axis().v,
                )),
            )?),
            Self::Harmonic(harmonic) => Self::Harmonic(HarmonicPcurve::try_new(
                scale(*harmonic.center()),
                scale(*harmonic.cosine()),
                scale(*harmonic.sine()),
            )?),
            Self::Hyperbolic(hyperbolic) => Self::Hyperbolic(HyperbolicPcurve::try_new(
                scale(*hyperbolic.center()),
                scale(*hyperbolic.cosine()),
                scale(*hyperbolic.sine()),
            )?),
            Self::Nurbs { nurbs } => {
                return nurbs
                    .edit_control_points(|point| {
                        *point = scale(*point);
                        Ok(())
                    })
                    .map_err(|error| error.to_string());
            }
            Self::Trimmed(trimmed) => {
                let mut basis = trimmed.basis.clone();
                basis.try_scale_coordinates(scales)?;
                Self::Trimmed(TrimmedPcurve::try_new(
                    trimmed.parameter_range,
                    trimmed.same_sense,
                    basis,
                )?)
            }
            Self::Offset(offset) => {
                if !isotropic {
                    return Err("offset coordinate scaling must be isotropic".into());
                }
                let mut basis = offset.basis.clone();
                basis.try_scale_coordinates(scales)?;
                Self::Offset(OffsetPcurve::try_new(offset.distance() * u_scale, basis)?)
            }
            Self::Transformed(placed) => {
                if !u_scale.is_finite() || !v_scale.is_finite() || u_scale == 0.0 || v_scale == 0.0
                {
                    return Err(
                        "transformed pcurve coordinate scales must be finite and nonzero".into(),
                    );
                }
                let mut rows = placed.transform.affine_rows();
                rows[0][1] *= u_scale / v_scale;
                rows[0][2] *= u_scale;
                rows[1][0] *= v_scale / u_scale;
                rows[1][2] *= v_scale;
                let transform =
                    Transform2::affine(rows).ok_or("scaled pcurve transform is invalid")?;
                let mut basis = placed.basis.clone();
                basis.try_scale_coordinates(scales)?;
                Self::Transformed(PlacedPcurve::try_new(basis, transform)?)
            }
            Self::PolarHarmonic(_) | Self::PolarNurbs { .. } | Self::SphericalGreatCircle(_) => {
                return if isotropic && u_scale == 1.0 {
                    Ok(())
                } else {
                    Err("polar pcurve coordinate scaling must be identity".into())
                };
            }
        };
        *self = scaled;
        Ok(())
    }

    /// Returns the origin and direction of a line-valued pcurve.
    ///
    /// Trimming and affine replicas preserve a line's parameterization. An
    /// offset does not preserve it because the offset is evaluated from the
    /// basis tangent, so it is deliberately excluded.
    pub fn line_parameters(&self) -> Option<(Point2, Point2)> {
        match self {
            Self::Line(line_pcurve) => {
                let origin = line_pcurve.origin().as_raw();
                let direction = line_pcurve.direction().as_raw();
                Some((*origin, *direction))
            }
            Self::Transformed(placed) => {
                let (origin, direction) = placed.basis().line_parameters()?;
                let transform = placed.transform();
                Some((
                    transform.apply_point(origin),
                    transform.apply_vector(direction),
                ))
            }
            Self::Trimmed(trimmed_pcurve) => {
                let basis = trimmed_pcurve.basis();
                basis.line_parameters()
            }
            Self::PolarHarmonic(_) => None,
            Self::PolarNurbs { .. } => None,
            Self::SphericalGreatCircle(_) => None,
            Self::Circle(_) => None,
            Self::Ellipse(_) => None,
            Self::Harmonic(_) => None,
            Self::Parabola(_) => None,
            Self::Hyperbola(_) => None,
            Self::Hyperbolic(_) => None,
            Self::Nurbs { .. } => None,
            Self::Offset(_) => None,
        }
    }
}

/// A pcurve carrier: the 2D image of a coedge in its face's surface parameter
/// space. Referenced by a coedge; the owning surface establishes whether a
/// parameter dimension is a length (relevant to unit scaling, see [F3D spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#5-topology-records)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Pcurve {
    /// Arena id.
    pub id: PcurveId,
    /// Parameter-space shape.
    pub geometry: PcurveGeometry,
    /// Source parameterization metadata.
    #[serde(default)]
    pub metadata: PcurveMetadata,
}

impl Pcurve {
    /// Native wrapper reversal, when the source stores one.
    pub fn wrapper_reversed(&self) -> Option<bool> {
        self.metadata.wrapper_reversed()
    }

    /// Four ASM booleans following an inline subtype scope.
    pub fn native_tail_flags(&self) -> Option<[bool; 4]> {
        self.metadata.native_tail_flags()
    }

    /// Directed native parameter interval on which this pcurve is evaluated.
    pub fn parameter_range(&self) -> Option<[f64; 2]> {
        self.metadata.parameter_range()
    }

    /// Parameter-space fit tolerance following a solved UV cache.
    pub fn fit_tolerance(&self) -> Option<f64> {
        self.metadata.fit_tolerance()
    }
}

/// Source-specific pcurve parameterization metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum PcurveMetadata {
    /// An ASM inline `exp_par_cur` record and its complete native tail.
    AsmInline {
        /// Complete inline record fields.
        form: PcurveInlineForm,
    },
    /// Metadata that does not assert the ASM inline-record contract.
    General {
        /// Optional fields carried without an inline-record claim.
        #[serde(default)]
        form: PcurveGeneralForm,
    },
}

impl Default for PcurveMetadata {
    fn default() -> Self {
        Self::General {
            form: PcurveGeneralForm::default(),
        }
    }
}

impl PcurveMetadata {
    /// The refusal of a parameter range with a non-finite endpoint.
    pub const NON_FINITE_PARAMETER_RANGE: &'static str =
        "pcurve parameter_range endpoints must be finite";
    /// The refusal of a fit tolerance that is not finite and non-negative.
    pub const INVALID_FIT_TOLERANCE: &'static str =
        "pcurve fit_tolerance must be finite and non-negative";

    /// Construct metadata without an ASM inline-record claim from admitted
    /// fields.
    #[must_use]
    pub const fn general(
        wrapper_reversed: Option<bool>,
        parameter_range: Option<FiniteVector<2>>,
        fit_tolerance: Option<FitTolerance>,
    ) -> Self {
        Self::General {
            form: PcurveGeneralForm::new(wrapper_reversed, parameter_range, fit_tolerance),
        }
    }

    /// Native wrapper reversal, when the source stores one.
    pub fn wrapper_reversed(&self) -> Option<bool> {
        match self {
            Self::AsmInline { form: inline } => Some(inline.wrapper_reversed),
            Self::General { form: general } => general.wrapper_reversed,
        }
    }

    /// Four ASM booleans following an inline subtype scope.
    pub fn native_tail_flags(&self) -> Option<[bool; 4]> {
        match self {
            Self::AsmInline { form: inline } => Some(inline.native_tail_flags),
            Self::General { .. } => None,
        }
    }

    /// Directed native parameter interval on which this pcurve is evaluated.
    pub fn parameter_range(&self) -> Option<[f64; 2]> {
        match self {
            Self::AsmInline { form: inline } => Some(inline.parameter_range),
            Self::General { form: general } => general.parameter_range,
        }
    }

    /// Parameter-space fit tolerance following a solved UV cache.
    pub fn fit_tolerance(&self) -> Option<f64> {
        match self {
            Self::AsmInline { form: inline } => Some(inline.fit_tolerance()),
            Self::General { form: general } => general.fit_tolerance(),
        }
    }
}

fn admit_pcurve_parameter_range(range: [f64; 2]) -> Result<FiniteVector<2>, &'static str> {
    FiniteVector::new(range).ok_or(PcurveMetadata::NON_FINITE_PARAMETER_RANGE)
}

fn admit_pcurve_fit_tolerance(value: f64) -> Result<FitTolerance, &'static str> {
    FitTolerance::try_new(value).map_err(|_| PcurveMetadata::INVALID_FIT_TOLERANCE)
}

/// The fields carried together by an ASM inline `exp_par_cur` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PcurveInlineFormWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PcurveInlineForm {
    /// Parameterization wrapper reversal.
    pub wrapper_reversed: bool,
    /// Four native booleans following the inline subtype scope.
    pub native_tail_flags: [bool; 4],
    parameter_range: [f64; 2],
    fit_tolerance: FitTolerance,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PcurveInlineFormWire {
    /// Parameterization wrapper reversal.
    wrapper_reversed: bool,
    /// Four native booleans following the inline subtype scope.
    native_tail_flags: [bool; 4],
    /// Directed native parameter interval.
    parameter_range: [f64; 2],
    /// Parameter-space fit tolerance.
    fit_tolerance: f64,
}

impl TryFrom<PcurveInlineFormWire> for PcurveInlineForm {
    type Error = &'static str;
    fn try_from(wire: PcurveInlineFormWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.wrapper_reversed,
            wire.native_tail_flags,
            wire.parameter_range,
            wire.fit_tolerance,
        )
    }
}

impl PcurveInlineForm {
    /// Construct inline metadata from admitted fields.
    #[must_use]
    pub const fn new(
        wrapper_reversed: bool,
        native_tail_flags: [bool; 4],
        parameter_range: FiniteVector<2>,
        fit_tolerance: FitTolerance,
    ) -> Self {
        Self {
            wrapper_reversed,
            native_tail_flags,
            parameter_range: parameter_range.get(),
            fit_tolerance,
        }
    }

    /// Admit inline metadata read from the wire, refusing a non-finite
    /// endpoint and then a tolerance that is not finite and non-negative.
    pub(crate) fn try_new(
        wrapper_reversed: bool,
        native_tail_flags: [bool; 4],
        parameter_range: [f64; 2],
        fit_tolerance: f64,
    ) -> Result<Self, &'static str> {
        Ok(Self::new(
            wrapper_reversed,
            native_tail_flags,
            admit_pcurve_parameter_range(parameter_range)?,
            admit_pcurve_fit_tolerance(fit_tolerance)?,
        ))
    }

    /// Parameter-space fit tolerance.
    #[must_use]
    pub const fn fit_tolerance(&self) -> f64 {
        self.fit_tolerance.get()
    }

    /// Replace the fit tolerance.
    pub fn set_fit_tolerance(&mut self, value: FitTolerance) {
        self.fit_tolerance = value;
    }

    /// Directed native parameter interval.
    #[must_use]
    pub const fn parameter_range(&self) -> [f64; 2] {
        self.parameter_range
    }
}

/// Pcurve metadata with no ASM inline-record contract.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PcurveGeneralFormWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PcurveGeneralForm {
    /// Source wrapper reversal, when stored independently of an ASM tail.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_wrapper_reversed"
    )]
    pub wrapper_reversed: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter_range"
    )]
    parameter_range: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fit_tolerance: Option<FitTolerance>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PcurveGeneralFormWire {
    /// Source wrapper reversal, when stored independently of an ASM tail.
    #[serde(default, deserialize_with = "deserialize_wrapper_reversed")]
    wrapper_reversed: Option<bool>,
    /// Directed native parameter interval.
    #[serde(default, deserialize_with = "deserialize_parameter_range")]
    parameter_range: Option<[f64; 2]>,
    /// Parameter-space fit tolerance.
    #[serde(
        default,
        deserialize_with = "deserialize_pcurve_general_form_wire_fit_tolerance"
    )]
    fit_tolerance: Option<f64>,
}

impl TryFrom<PcurveGeneralFormWire> for PcurveGeneralForm {
    type Error = &'static str;
    fn try_from(wire: PcurveGeneralFormWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.wrapper_reversed,
            wire.parameter_range,
            wire.fit_tolerance,
        )
    }
}

impl PcurveGeneralForm {
    /// Construct general metadata from admitted fields.
    #[must_use]
    pub const fn new(
        wrapper_reversed: Option<bool>,
        parameter_range: Option<FiniteVector<2>>,
        fit_tolerance: Option<FitTolerance>,
    ) -> Self {
        Self {
            wrapper_reversed,
            parameter_range: match parameter_range {
                Some(range) => Some(range.get()),
                None => None,
            },
            fit_tolerance,
        }
    }

    /// Admit general metadata read from the wire, refusing a non-finite
    /// endpoint and then a tolerance that is not finite and non-negative.
    pub(crate) fn try_new(
        wrapper_reversed: Option<bool>,
        parameter_range: Option<[f64; 2]>,
        fit_tolerance: Option<f64>,
    ) -> Result<Self, &'static str> {
        Ok(Self::new(
            wrapper_reversed,
            parameter_range
                .map(admit_pcurve_parameter_range)
                .transpose()?,
            fit_tolerance.map(admit_pcurve_fit_tolerance).transpose()?,
        ))
    }

    /// Parameter-space fit tolerance.
    #[must_use]
    pub fn fit_tolerance(&self) -> Option<f64> {
        self.fit_tolerance.map(FitTolerance::get)
    }

    /// Replace or clear the fit tolerance.
    pub fn set_fit_tolerance(&mut self, value: Option<FitTolerance>) {
        self.fit_tolerance = value;
    }

    /// Directed native parameter interval.
    #[must_use]
    pub const fn parameter_range(&self) -> Option<[f64; 2]> {
        self.parameter_range
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_wrapper_reversed, bool, "wrapper_reversed");
cadmpeg_core::named_optional_field!(deserialize_parameter_range, [f64; 2], "parameter_range");
cadmpeg_core::named_optional_field!(
    deserialize_pcurve_general_form_wire_fit_tolerance,
    f64,
    "fit_tolerance"
);

#[cfg(test)]
mod tests;
