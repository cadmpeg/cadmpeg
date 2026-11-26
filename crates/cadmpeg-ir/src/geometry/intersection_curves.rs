// SPDX-License-Identifier: Apache-2.0
//! Intersection curve supports and tolerant intersection constructions.

use super::pcurve::PcurveGeometry;
use super::procedural_curves::{require_mapped_interval, DirectedParameterRange, SupportPcurve};
use crate::features::geometry::FinitePoint3;
use crate::ids::SurfaceId;
use crate::math::sum::{fast_dot, ExactSignedSum};
use crate::math::Point3;
use crate::scalar::{FiniteReal, NonNegativeReal};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One paired surface and parameter-space curve in an intcurve construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IntcurveSupportSide {
    /// Supporting surface, absent for the native `null_surface` sentinel.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_surface"
    )]
    pub surface: Option<SurfaceId>,
    /// UV curve on `surface`, absent for the native `nullbs` sentinel.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_intcurve_support_side_pcurve"
    )]
    pub pcurve: Option<SupportPcurve>,
}

impl IntcurveSupportSide {
    /// Return the mapped pcurve interval, when this side's pcurve states one.
    #[must_use]
    pub fn pcurve_parameter_range(&self) -> Option<DirectedParameterRange> {
        self.pcurve
            .as_ref()
            .and_then(|pcurve| pcurve.parameter_range)
    }

    /// Map one solved-curve parameter into this side's pcurve parameter.
    ///
    /// Returns `None` when this side has no pcurve, an explicit map has a
    /// zero-width solved interval, or the mapped parameter cannot be
    /// represented as finite. Each branch admits the parameter it returns
    /// where it computes it.
    #[must_use]
    pub fn pcurve_parameter(
        &self,
        solved_parameter_range: crate::topology::ParameterInterval,
        parameter: f64,
    ) -> Option<FiniteReal> {
        let admitted_parameter = FiniteReal::new(parameter)?;
        let Some(pcurve_interval) = self.pcurve.as_ref()?.parameter_range else {
            return Some(admitted_parameter);
        };
        let [pcurve_start, pcurve_end] = pcurve_interval.finite_endpoints();
        let pcurve_range = pcurve_interval.endpoints();
        let solved_parameter_range = solved_parameter_range.endpoints();
        let solved_span = solved_parameter_range[1] - solved_parameter_range[0];
        if solved_span == 0.0 {
            return None;
        }
        if parameter == solved_parameter_range[0] {
            return Some(pcurve_start);
        }
        if parameter == solved_parameter_range[1] {
            return Some(pcurve_end);
        }
        let offset = parameter - solved_parameter_range[0];
        let mapped_span = pcurve_range[1] - pcurve_range[0];
        if solved_span.is_finite() && offset.is_finite() && mapped_span.is_finite() {
            let fraction = offset / solved_span;
            let advance = fraction * mapped_span;
            if fraction.is_normal() && advance.is_normal() {
                if let Some(mapped) = fast_dot(
                    [pcurve_range[0], advance],
                    [1.0, 1.0],
                    [pcurve_range[0], advance],
                ) {
                    return Some(mapped);
                }
            }
        }
        // Evaluate the affine numerator before division. A tiny fraction can
        // underflow even when the final mapped parameter is representable.
        let mut numerator = ExactSignedSum::default();
        numerator.add_product(pcurve_range[0], solved_parameter_range[1]);
        numerator.add_product(-pcurve_range[0], parameter);
        numerator.add_product(pcurve_range[1], parameter);
        numerator.add_product(-pcurve_range[1], solved_parameter_range[0]);
        let mut denominator = ExactSignedSum::default();
        denominator.add_product(solved_parameter_range[1], 1.0);
        denominator.add_product(-solved_parameter_range[0], 1.0);
        let denominator = denominator.finish()?;
        numerator.finish().map_or(Some(FiniteReal::ZERO), |value| {
            value.quotient(denominator).ok()
        })
    }
}

/// Version-stamp prefix and unbounded interval carried by the stamped
/// `law_int_cur` serializer form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LawCurveVersionFormWire")]
pub struct LawCurveVersionForm {
    /// Serializer version stamp emitted after the subtype name.
    pub(super) stamp: i64,
    /// Native enum following the version stamp.
    pub(super) post_enum: i64,
    /// Solved-curve interval endpoints; `None` records an unbounded bound.
    pub(super) parameter_range: [Option<FiniteReal>; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct LawCurveVersionFormWire {
    /// Serializer version stamp emitted after the subtype name.
    pub(super) stamp: i64,
    /// Native enum following the version stamp.
    pub(super) post_enum: i64,
    /// Solved-curve interval endpoints; `None` records an unbounded bound.
    pub(super) parameter_range: [Option<f64>; 2],
}

impl TryFrom<LawCurveVersionFormWire> for LawCurveVersionForm {
    type Error = &'static str;

    fn try_from(wire: LawCurveVersionFormWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.stamp, wire.post_enum, wire.parameter_range)
    }
}

impl LawCurveVersionForm {
    /// Admit a stamped law-curve form whose interval endpoints are finite.
    pub fn try_new(
        stamp: i64,
        post_enum: i64,
        parameter_range: [Option<f64>; 2],
    ) -> Result<Self, &'static str> {
        let parameter_range = FiniteReal::optional(parameter_range)
            .ok_or("law curve parameter_range bounds must be finite")?;
        Ok(Self {
            stamp,
            post_enum,
            parameter_range,
        })
    }

    /// Serializer version stamp emitted after the subtype name.
    #[must_use]
    pub const fn stamp(&self) -> i64 {
        self.stamp
    }

    /// Native enum following the version stamp.
    #[must_use]
    pub const fn post_enum(&self) -> i64 {
        self.post_enum
    }

    /// Solved-curve interval endpoints; `None` records an unbounded bound.
    #[must_use]
    pub const fn parameter_range(&self) -> [Option<FiniteReal>; 2] {
        self.parameter_range
    }
}

/// Shared support surfaces, UV curves, interval, and discontinuity arrays of a native intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[serde(try_from = "IntcurveSupportContextWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct IntcurveSupportContext {
    pub(super) sides: [IntcurveSupportSide; 2],
    pub(super) parameter_range: crate::topology::ParameterInterval,
    pub(super) discontinuities: [Vec<FiniteReal>; 3],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct IntcurveSupportContextWire {
    /// Ordered support sides.
    pub(super) sides: [IntcurveSupportSide; 2],
    /// Solved-curve interval.
    pub(super) parameter_range: [f64; 2],
    /// Ordered discontinuity arrays.
    pub(super) discontinuities: [Vec<f64>; 3],
}

impl TryFrom<IntcurveSupportContextWire> for IntcurveSupportContext {
    type Error = &'static str;

    fn try_from(wire: IntcurveSupportContextWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.sides, wire.parameter_range, wire.discontinuities)
    }
}

impl IntcurveSupportContext {
    /// Construct a support context with a finite ordered interval and finite discontinuities.
    pub fn try_new(
        sides: [IntcurveSupportSide; 2],
        parameter_range: [f64; 2],
        discontinuities: [Vec<f64>; 3],
    ) -> Result<Self, &'static str> {
        let parameter_range = crate::topology::ParameterInterval::new(parameter_range)
            .map_err(|_| "support context parameter_range must be finite and ordered")?;
        require_mapped_interval(&sides, parameter_range)?;
        let discontinuities = FiniteReal::lanes(discontinuities)
            .ok_or("support context discontinuities must be finite")?;
        Ok(Self {
            sides,
            parameter_range,
            discontinuities,
        })
    }

    /// Construct a support context from an admitted interval and admitted
    /// discontinuities. Only the interval's width against an explicit pcurve
    /// mapping is tested.
    pub fn from_parts(
        sides: [IntcurveSupportSide; 2],
        parameter_range: crate::topology::ParameterInterval,
        discontinuities: [Vec<FiniteReal>; 3],
    ) -> Result<Self, &'static str> {
        require_mapped_interval(&sides, parameter_range)?;
        Ok(Self {
            sides,
            parameter_range,
            discontinuities,
        })
    }

    /// Build a support context over an increasing interval with no
    /// discontinuities. The interval type states finite, ordered and nonzero
    /// endpoints, which is the whole interval condition of [`Self::try_new`],
    /// so nothing is checked.
    #[must_use]
    pub fn over_interval(
        sides: [IntcurveSupportSide; 2],
        parameter_range: crate::topology::IncreasingParameterInterval,
    ) -> Self {
        Self {
            sides,
            parameter_range: parameter_range.into(),
            discontinuities: std::array::from_fn(|_| Vec::new()),
        }
    }

    /// Set a support surface without changing its pcurve mapping.
    pub fn set_surface(&mut self, side: usize, surface: Option<SurfaceId>) {
        self.sides[side].surface = surface;
    }

    /// Set a support pcurve with the solved-curve parameterization.
    pub fn set_unmapped_pcurve(&mut self, side: usize, geometry: Option<PcurveGeometry>) {
        self.sides[side].pcurve = geometry.map(SupportPcurve::from);
    }

    /// Copy the support pcurve of side `source` onto side `target`.
    pub fn copy_pcurve(&mut self, source: usize, target: usize) {
        if source == target {
            return;
        }
        let copied = self.sides[source]
            .pcurve
            .as_ref()
            .map(|pcurve| SupportPcurve::new(pcurve.geometry.clone(), pcurve.parameter_range));
        self.sides[target].pcurve = copied;
    }

    /// Return the ordered support sides.
    #[must_use]
    pub const fn sides(&self) -> &[IntcurveSupportSide; 2] {
        &self.sides
    }

    /// Return the solved-curve interval.
    #[must_use]
    pub const fn parameter_range(&self) -> crate::topology::ParameterInterval {
        self.parameter_range
    }

    /// Return the ordered discontinuity arrays.
    #[must_use]
    pub const fn discontinuities(&self) -> &[Vec<FiniteReal>; 3] {
        &self.discontinuities
    }
}

/// Finite endpoint witnesses and distinct supports of a tolerant intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TolerantIntersectionConstructionWire")]
pub struct TolerantIntersectionConstruction {
    pub(super) supports: [SurfaceId; 2],
    pub(super) endpoints: [FinitePoint3; 2],
    pub(super) tolerance: NonNegativeReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct TolerantIntersectionConstructionWire {
    /// Distinct support surfaces of the intersection.
    pub(super) supports: [SurfaceId; 2],
    /// Model-space endpoint witnesses.
    pub(super) endpoints: [Point3; 2],
    /// Finite non-negative intersection tolerance.
    pub(super) tolerance: f64,
}

impl TryFrom<TolerantIntersectionConstructionWire> for TolerantIntersectionConstruction {
    type Error = &'static str;
    fn try_from(wire: TolerantIntersectionConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.supports, wire.endpoints, wire.tolerance)
    }
}

impl TolerantIntersectionConstruction {
    /// Admit distinct supports, finite endpoints, and a finite non-negative tolerance.
    pub fn try_new(
        supports: [SurfaceId; 2],
        endpoints: [Point3; 2],
        tolerance: f64,
    ) -> Result<Self, &'static str> {
        if supports[0] == supports[1] {
            return Err("tolerant intersection supports must be distinct");
        }
        let [start, end] = endpoints.map(FinitePoint3::new);
        let (Some(start), Some(end)) = (start, end) else {
            return Err("tolerant intersection endpoints must be finite");
        };
        let endpoints = [start, end];
        let tolerance = NonNegativeReal::new(tolerance)
            .ok_or("tolerant intersection tolerance must be finite and non-negative")?;
        Ok(Self {
            supports,
            endpoints,
            tolerance,
        })
    }

    /// Admit distinct supports with admitted endpoints and tolerance. The
    /// endpoint and tolerance types state their conditions, so only the
    /// distinctness of the supports is checked.
    pub fn from_parts(
        supports: [SurfaceId; 2],
        endpoints: [FinitePoint3; 2],
        tolerance: NonNegativeReal,
    ) -> Result<Self, &'static str> {
        if supports[0] == supports[1] {
            return Err("tolerant intersection supports must be distinct");
        }
        Ok(Self {
            supports,
            endpoints,
            tolerance,
        })
    }

    /// Return the supports.
    #[must_use]
    pub const fn supports(&self) -> &[SurfaceId; 2] {
        &self.supports
    }

    /// Return the endpoints.
    #[must_use]
    pub const fn endpoints(&self) -> &[FinitePoint3; 2] {
        &self.endpoints
    }

    /// Return the admitted tolerance.
    #[must_use]
    pub const fn tolerance(&self) -> NonNegativeReal {
        self.tolerance
    }
}

/// Complete neutral parameterization of one topology-bounded intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TolerantIntersectionParameterizationWire")]
pub struct TolerantIntersectionParameterization {
    /// Coincident support charts in support order.
    pub pcurves: [PcurveGeometry; 2],
    pub(super) parameter_range: crate::topology::IncreasingParameterInterval,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct TolerantIntersectionParameterizationWire {
    /// Coincident support charts in support order.
    pub(super) pcurves: [PcurveGeometry; 2],
    /// Common finite solved-curve interval.
    pub(super) parameter_range: [f64; 2],
}

impl TryFrom<TolerantIntersectionParameterizationWire> for TolerantIntersectionParameterization {
    type Error = &'static str;
    fn try_from(wire: TolerantIntersectionParameterizationWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.pcurves, wire.parameter_range)
    }
}

impl TolerantIntersectionParameterization {
    /// Admit a finite strictly increasing solved-curve interval.
    pub fn try_new(
        pcurves: [PcurveGeometry; 2],
        parameter_range: [f64; 2],
    ) -> Result<Self, &'static str> {
        let parameter_range = crate::topology::IncreasingParameterInterval::new(parameter_range)
            .ok_or(
                "tolerant intersection parameter_range must be finite and strictly increasing",
            )?;
        Ok(Self {
            pcurves,
            parameter_range,
        })
    }

    /// Common finite solved-curve interval.
    #[must_use]
    pub const fn parameter_range(&self) -> crate::topology::IncreasingParameterInterval {
        self.parameter_range
    }
}

cadmpeg_core::named_optional_field!(deserialize_surface, SurfaceId, "surface");

cadmpeg_core::named_optional_field!(
    deserialize_intcurve_support_side_pcurve,
    SupportPcurve,
    "pcurve"
);
