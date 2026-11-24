// SPDX-License-Identifier: Apache-2.0
//! Curve offset sides, ranges, coordinates, and distance laws.

use super::legacy_cache::ProceduralGeometryError;
use crate::ids::{CurveId, SurfaceId};
use crate::math::Vector3;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Orientation carrier of a planar curve offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OffsetSide<V = Vector3> {
    /// Unit plane normal defining the positive offset side.
    PlaneNormal {
        /// Unit plane normal.
        normal: V,
    },
    /// Explicit offset direction, optionally constrained to a support surface.
    Direction {
        /// Nonzero offset direction.
        direction: V,
        /// Support surface within which the offset is measured.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset_side_support"
        )]
        support: Option<SurfaceId>,
    },
}

/// Parameter interval and optional variable-distance law of a curve offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveOffsetRange<R = f64> {
    /// Constant-distance offset over a retained source interval.
    Uniform {
        /// Parameter interval on the source curve.
        #[serde(deserialize_with = "deserialize_curve_offset_interval")]
        parameter_range: crate::topology::IncreasingParameterInterval,
    },
    /// Variable-distance offset over the interval used by its law.
    Variable {
        /// Parameter interval on the source curve.
        #[serde(deserialize_with = "deserialize_curve_offset_interval")]
        parameter_range: crate::topology::IncreasingParameterInterval,
        /// Variable signed-distance law.
        distance_law: CurveOffsetDistanceLaw<R>,
    },
}

/// The refusal of a curve offset whose distance, side, range or law is
/// invalid.
pub(crate) const INVALID_CURVE_OFFSET: &str =
    "curve offset distance, side, range, or law is invalid";

/// Admit a curve-offset interval with the curve-offset refusal.
pub(super) fn admit_curve_offset_interval(
    range: [f64; 2],
) -> Result<crate::topology::IncreasingParameterInterval, ProceduralGeometryError> {
    crate::topology::IncreasingParameterInterval::new(range)
        .ok_or(ProceduralGeometryError::Payload(INVALID_CURVE_OFFSET))
}

/// Read a curve-offset interval with the curve-offset refusal.
pub(super) fn deserialize_curve_offset_interval<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<crate::topology::IncreasingParameterInterval, D::Error> {
    crate::topology::IncreasingParameterInterval::new(<[f64; 2]>::deserialize(deserializer)?)
        .ok_or_else(|| serde::de::Error::custom(INVALID_CURVE_OFFSET))
}

impl CurveOffsetRange {
    /// Admit a constant-distance range over a strictly increasing source
    /// interval. The refusal is the one
    /// [`OffsetCurveConstruction::try_new`](crate::geometry::curve_payloads::OffsetCurveConstruction::try_new)
    /// states for an invalid offset.
    pub fn uniform(parameter_range: [f64; 2]) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Uniform {
            parameter_range: admit_curve_offset_interval(parameter_range)?,
        })
    }

    /// Admit a variable-distance range over a strictly increasing source
    /// interval, with the refusal of [`CurveOffsetRange::uniform`].
    pub fn variable(
        parameter_range: [f64; 2],
        distance_law: CurveOffsetDistanceLaw,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Variable {
            parameter_range: admit_curve_offset_interval(parameter_range)?,
            distance_law,
        })
    }
}

/// Independent variable used by a curve-offset distance law.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CurveOffsetLawBasis {
    /// Distance measured along the source curve from the offset interval start.
    ArcLength,
    /// Native source-curve parameter.
    Parameter,
}

/// A one-based coordinate of a curve-offset distance function.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "u8", into = "u8")]
pub struct CurveOffsetCoordinate(u8);

impl CurveOffsetCoordinate {
    /// Admit coordinate one, two, or three.
    pub fn try_new(coordinate: u8) -> Result<Self, &'static str> {
        match coordinate {
            1..=3 => Ok(Self(coordinate)),
            _ => Err("curve offset coordinate must be 1, 2, or 3"),
        }
    }

    /// One-based coordinate number.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for CurveOffsetCoordinate {
    type Error = &'static str;
    fn try_from(coordinate: u8) -> Result<Self, Self::Error> {
        Self::try_new(coordinate)
    }
}

impl From<CurveOffsetCoordinate> for u8 {
    fn from(coordinate: CurveOffsetCoordinate) -> Self {
        coordinate.get()
    }
}

/// Variable signed distance law for a planar curve offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveOffsetDistanceLaw<R = f64> {
    /// Linear interpolation between two distance controls.
    Linear {
        /// Independent-variable interpretation.
        basis: CurveOffsetLawBasis,
        /// Ordered signed distances in document length units.
        distances: [R; 2],
        /// Ordered arc-length or neutral carrier-parameter controls.
        #[serde(deserialize_with = "deserialize_curve_offset_interval")]
        control_range: crate::topology::IncreasingParameterInterval,
    },
    /// One coordinate of another curve defines the signed distance.
    Coordinate {
        /// Curve carrying the distance function.
        function: CurveId,
        /// One-based coordinate number on `function`.
        coordinate: CurveOffsetCoordinate,
        /// Independent-variable interpretation.
        basis: CurveOffsetLawBasis,
        /// Function parameter at zero source parameter or arc length.
        function_parameter_offset: R,
        /// Function-parameter change per neutral source parameter or length unit.
        function_parameter_scale: R,
    },
}

impl CurveOffsetDistanceLaw {
    /// Admit a linear law over strictly increasing controls, with the refusal
    /// of [`CurveOffsetRange::uniform`]. The distances are checked by the
    /// offset construction.
    pub fn linear(
        basis: CurveOffsetLawBasis,
        distances: [f64; 2],
        control_range: [f64; 2],
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Linear {
            basis,
            distances,
            control_range: admit_curve_offset_interval(control_range)?,
        })
    }
}

cadmpeg_core::named_optional_field!(deserialize_offset_side_support, SurfaceId, "support");
