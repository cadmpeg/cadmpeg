// SPDX-License-Identifier: Apache-2.0
//! Preserve whether the authored radial control is a radius or a diameter.

use super::{Dimension, DimensionResolutionError, DimensionValue, ParameterValue};
use crate::scalar::Length;
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_core::{decode::DecodeContext, CodecError};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A radial dimension with its actual authored measurement convention.
/// A diameter binding stores the diameter parameter identity, without a radius cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RadialDimension<Q = Length> {
    /// The source or author controls the radius directly.
    Radius(Dimension<Q>),
    /// The source or author controls the diameter directly.
    Diameter(Dimension<Q>),
}

impl<Q> RadialDimension<Q> {
    /// The sole canonical control, preserving its parameter binding.
    pub const fn dimension(&self) -> &Dimension<Q> {
        match self {
            Self::Radius(value) | Self::Diameter(value) => value,
        }
    }

    /// Edit the sole canonical control in this standalone value.
    pub fn dimension_mut(&mut self) -> &mut Dimension<Q> {
        match self {
            Self::Radius(value) | Self::Diameter(value) => value,
        }
    }

    /// Transform only an authored literal, retaining both convention and binding.
    pub fn try_map_literal<R, E>(
        self,
        map: impl FnOnce(Q) -> Result<R, E>,
    ) -> Result<RadialDimension<R>, E> {
        Ok(match self {
            Self::Radius(value) => RadialDimension::Radius(value.try_map_literal(map)?),
            Self::Diameter(value) => RadialDimension::Diameter(value.try_map_literal(map)?),
        })
    }
}

impl<Q: DimensionValue> RadialDimension<Q> {
    fn radius_value(&self, value: Q) -> Option<Q> {
        if matches!(self, Self::Radius(_)) {
            return Some(value);
        }
        let ParameterValue::Length(value) = value.to_parameter_value()? else {
            return None;
        };
        Q::from_parameter_value(&ParameterValue::Length(Length::new(value.get() / 2.0)?))
    }

    /// Derive the radius when the canonical control is a literal.
    pub fn literal_radius(&self) -> Option<Q> {
        self.radius_value(self.dimension().literal()?.clone())
    }

    /// Derive the current radius from the sole literal or parameter cache.
    pub fn resolved_radius(&self, ir: &crate::CadIr) -> Option<Q> {
        self.radius_value(ir.dimension_value(self.dimension())?)
    }
}

impl<Q> From<Dimension<Q>> for RadialDimension<Q> {
    fn from(value: Dimension<Q>) -> Self {
        Self::Radius(value)
    }
}
impl<Q> From<Q> for RadialDimension<Q> {
    fn from(value: Q) -> Self {
        Self::Radius(Dimension::Literal(value))
    }
}

impl<Q: RewriteIdentities + DimensionValue> RewriteIdentities for RadialDimension<Q> {
    fn validate_dimensions(
        &self,
        ir: &crate::CadIr,
        ctx: &DecodeContext<'_>,
        errors: &mut Vec<DimensionResolutionError>,
    ) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
        self.dimension().validate_dimensions(ir, ctx, errors)
    }
    fn resolve_dimensions(
        self,
        ir: &crate::CadIr,
        ctx: &DecodeContext<'_>,
    ) -> Result<Self, DimensionResolutionError> {
        let value = self.dimension().clone().resolve_dimensions(ir, ctx)?;
        let value = value.literal().cloned().ok_or_else(|| {
            DimensionResolutionError::InvalidDefinition {
                reason: "resolved radial control has no literal evaluation".into(),
            }
        })?;
        let radius = self.radius_value(value).ok_or_else(|| {
            DimensionResolutionError::InvalidDefinition {
                reason:
                    "radial control cannot produce a representable radius of the required quantity"
                        .into(),
            }
        })?;
        // This is a temporary evaluated encoder/validation projection. The
        // document retains its original Radius/Diameter arm and sole control.
        Ok(Self::Radius(Dimension::Literal(radius)))
    }
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.dimension().visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError>
    where
        F: FnMut(&str) -> Result<String, CodecError>,
    {
        Ok(match self {
            Self::Radius(value) => Self::Radius(value.rewrite_identities(ctx, map)?),
            Self::Diameter(value) => Self::Diameter(value.rewrite_identities(ctx, map)?),
        })
    }
}

#[cfg(test)]
mod tests;
