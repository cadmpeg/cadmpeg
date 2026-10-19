// SPDX-License-Identifier: Apache-2.0
//! One canonical value for literal and parameter-bound dimensions.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{DesignParameter, Expression, ParameterId, ParameterValue};
use crate::scalar::{
    Angle, FiniteReal, Fraction, InteriorAngle, Length, NonNegativeLength, NonZeroLength,
    NonZeroReal, PositiveAngle, PositiveLength, PositiveReal, SlopeAngle,
};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_core::{decode::DecodeContext, CodecError};

mod radial;
pub use radial::RadialDimension;
mod ranges;
pub use ranges::{FeatureAngularRange, FeatureScalarRange};

/// An authored dimension or a reference to its single canonical parameter.
/// A bound dimension stores no evaluated value of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Dimension<Q> {
    /// A dimension with no parameter binding.
    Literal(Q),
    /// The parameter whose cached evaluation supplies this dimension.
    Parameter(ParameterId),
}

impl<Q> Dimension<Q> {
    /// The literal value, if this dimension is unbound.
    pub const fn literal(&self) -> Option<&Q> {
        match self {
            Self::Literal(value) => Some(value),
            Self::Parameter(_) => None,
        }
    }
    /// The canonical parameter, if this dimension is bound.
    pub const fn parameter(&self) -> Option<&ParameterId> {
        match self {
            Self::Parameter(id) => Some(id),
            Self::Literal(_) => None,
        }
    }
    /// Transform an unbound value while leaving a binding intact.
    pub fn map_literal<R>(self, map: impl FnOnce(Q) -> R) -> Dimension<R> {
        match self {
            Self::Literal(value) => Dimension::Literal(map(value)),
            Self::Parameter(id) => Dimension::Parameter(id),
        }
    }
    /// Fallibly transform an unbound value while leaving a binding intact.
    pub fn try_map_literal<R, E>(
        self,
        map: impl FnOnce(Q) -> Result<R, E>,
    ) -> Result<Dimension<R>, E> {
        Ok(match self {
            Self::Literal(value) => Dimension::Literal(map(value)?),
            Self::Parameter(id) => Dimension::Parameter(id),
        })
    }
}
impl<Q: Default> Default for Dimension<Q> {
    fn default() -> Self {
        Self::Literal(Q::default())
    }
}

impl<Q> From<Q> for Dimension<Q> {
    fn from(value: Q) -> Self {
        Self::Literal(value)
    }
}

/// Quantity-preserving conversion between a dimension and a parameter cache.
/// Admission of positive, nonzero and integer quantities remains checked.
pub trait DimensionValue: Clone {
    /// Resolve a cache of the same quantity, retaining this type's invariants.
    fn from_parameter_value(value: &ParameterValue) -> Option<Self>;
    /// Store this quantity as its canonical parameter value.
    fn to_parameter_value(&self) -> Option<ParameterValue>;
    /// Resolve a parameter for a temporary writer projection. Scalar quantities
    /// use its cache; expression-valued native operands use its canonical tokens.
    fn resolve_parameter(parameter: &DesignParameter) -> Option<Self> {
        parameter
            .value
            .as_ref()
            .and_then(Self::from_parameter_value)
    }
}

impl<Q: DimensionValue> Dimension<Q> {
    /// Resolve a literal or the canonical cache from a typed parameter arena.
    pub fn value_from(&self, parameters: &[DesignParameter]) -> Option<Q> {
        match self {
            Self::Literal(value) => Some(value.clone()),
            Self::Parameter(id) => parameters
                .iter()
                .find(|parameter| parameter.id == *id)
                .and_then(|parameter| parameter.value.as_ref())
                .and_then(Q::from_parameter_value),
        }
    }
}

macro_rules! quantity {
    ($variant:ident, $base:ty; $($ty:ty),+ $(,)?) => {$ (
        impl DimensionValue for $ty {
            fn from_parameter_value(value: &ParameterValue) -> Option<Self> {
                let ParameterValue::$variant(value) = value else { return None; };
                Self::new(value.get())
            }
            fn to_parameter_value(&self) -> Option<ParameterValue> {
                Some(ParameterValue::$variant(<$base>::new(self.get())?))
            }
        }
    )+};
}
quantity!(Length, Length; Length, NonZeroLength, PositiveLength, NonNegativeLength);
quantity!(Angle, Angle; Angle, PositiveAngle, InteriorAngle, SlopeAngle);
quantity!(Real, FiniteReal; FiniteReal, PositiveReal, NonZeroReal, Fraction);

impl DimensionValue for i64 {
    fn from_parameter_value(value: &ParameterValue) -> Option<Self> {
        match value {
            ParameterValue::Integer(value) => Some(*value),
            _ => None,
        }
    }
    fn to_parameter_value(&self) -> Option<ParameterValue> {
        Some(ParameterValue::Integer(*self))
    }
}
macro_rules! integer {
    ($($ty:ty),+ $(,)?) => {$ (
        impl DimensionValue for $ty {
            fn from_parameter_value(value: &ParameterValue) -> Option<Self> {
                Self::try_from(i64::from_parameter_value(value)?).ok()
            }
            fn to_parameter_value(&self) -> Option<ParameterValue> {
                Some(ParameterValue::Integer(i64::try_from(*self).ok()?))
            }
        }
    )+};
}
integer!(u32, u64, usize, i32);
impl DimensionValue for std::num::NonZeroU32 {
    fn from_parameter_value(value: &ParameterValue) -> Option<Self> {
        Self::new(u32::from_parameter_value(value)?)
    }
    fn to_parameter_value(&self) -> Option<ParameterValue> {
        self.get().to_parameter_value()
    }
}

impl<Q: RewriteIdentities + DimensionValue> RewriteIdentities for Dimension<Q> {
    fn validate_dimensions(
        &self,
        ir: &crate::CadIr,
        _ctx: &DecodeContext<'_>,
        errors: &mut Vec<DimensionResolutionError>,
    ) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
        if let Self::Parameter(id) = self {
            match ir
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id == *id)
            {
                None => errors.push(DimensionResolutionError::UnknownParameter {
                    parameter: id.clone(),
                }),
                Some(parameter)
                    if parameter.value.is_some() && Q::resolve_parameter(parameter).is_none() =>
                {
                    errors.push(DimensionResolutionError::WrongQuantity {
                        parameter: id.clone(),
                        expected: std::any::type_name::<Q>().into(),
                    });
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn resolve_dimensions(
        self,
        ir: &crate::CadIr,
        _ctx: &DecodeContext<'_>,
    ) -> Result<Self, DimensionResolutionError> {
        match self {
            Self::Literal(value) => Ok(Self::Literal(value)),
            Self::Parameter(id) => {
                let parameter = ir
                    .model
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == id)
                    .ok_or_else(|| DimensionResolutionError::UnknownParameter {
                        parameter: id.clone(),
                    })?;
                let value = Q::resolve_parameter(parameter).ok_or_else(|| {
                    if parameter.value.is_none() {
                        DimensionResolutionError::UnevaluatedParameter {
                            parameter: id.clone(),
                        }
                    } else {
                        DimensionResolutionError::WrongQuantity {
                            parameter: id.clone(),
                            expected: std::any::type_name::<Q>().into(),
                        }
                    }
                })?;
                Ok(Self::Literal(value))
            }
        }
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            Self::Literal(value) => value.visit_identity_references(ctx, visitor),
            Self::Parameter(id) => id.visit_identity_references(ctx, visitor),
        }
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
            Self::Literal(value) => Self::Literal(value.rewrite_identities(ctx, map)?),
            Self::Parameter(id) => Self::Parameter(id.rewrite_identities(ctx, map)?),
        })
    }
}

impl DimensionValue for super::PolygonSideCount {
    fn from_parameter_value(value: &ParameterValue) -> Option<Self> {
        Self::new(u32::from_parameter_value(value)?)
    }
    fn to_parameter_value(&self) -> Option<ParameterValue> {
        self.0.to_parameter_value()
    }
}

/// A bound dimension that cannot supply the requested typed writer value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DimensionResolutionError {
    /// The canonical parameter is absent.
    UnknownParameter {
        /// Missing canonical parameter.
        parameter: ParameterId,
    },
    /// The canonical parameter has no evaluated value.
    UnevaluatedParameter {
        /// Parameter with no cached value.
        parameter: ParameterId,
    },
    /// The cache does not satisfy the dimension's quantity or admission rules.
    WrongQuantity {
        /// Parameter whose cached quantity is incompatible.
        parameter: ParameterId,
        /// Expected admitted dimensional type.
        expected: String,
    },
    /// The admitted definition could not be projected for an encoder.
    InvalidDefinition {
        /// Invalid definition detail.
        reason: String,
    },
    /// The active traversal's resource policy refused the projection.
    ResourceLimit(cadmpeg_core::decode::ResourceLimit),
}
impl From<cadmpeg_core::decode::ResourceLimit> for DimensionResolutionError {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::ResourceLimit(limit)
    }
}
impl std::fmt::Display for DimensionResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownParameter { parameter } => {
                write!(formatter, "unknown dimensional parameter {parameter}")
            }
            Self::UnevaluatedParameter { parameter } => write!(
                formatter,
                "dimensional parameter {parameter} is unevaluated"
            ),
            Self::WrongQuantity {
                parameter,
                expected,
            } => write!(
                formatter,
                "dimensional parameter {parameter} does not supply {expected}"
            ),
            Self::InvalidDefinition { reason } => formatter.write_str(reason),
            Self::ResourceLimit(limit) => {
                std::fmt::Display::fmt(&CodecError::ResourceLimit(*limit), formatter)
            }
        }
    }
}
impl std::error::Error for DimensionResolutionError {}

impl DimensionValue for Expression {
    fn from_parameter_value(_value: &ParameterValue) -> Option<Self> {
        None
    }
    fn to_parameter_value(&self) -> Option<ParameterValue> {
        None
    }
    fn resolve_parameter(parameter: &DesignParameter) -> Option<Self> {
        Some(parameter.expression.clone())
    }
}

/// Collect the distinct canonical parameters bound to dimensions, excluding
/// references inside expressions (which have their own dependency iterator).
pub(crate) fn bound_parameter_ids(
    value: &impl Serialize,
) -> Result<Vec<ParameterId>, DimensionResolutionError> {
    fn walk(value: &serde_json::Value, ids: &mut std::collections::BTreeSet<ParameterId>) {
        match value {
            serde_json::Value::Object(fields)
                if fields.len() == 2
                    && fields.get("kind").and_then(serde_json::Value::as_str)
                        == Some("parameter") =>
            {
                if let Some(id) = fields
                    .get("value")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|id| ParameterId::try_from(id).ok())
                {
                    ids.insert(id);
                }
            }
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    if key != "tokens" {
                        walk(value, ids);
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    walk(value, ids);
                }
            }
            _ => {}
        }
    }
    let value = serde_json::to_value(value).map_err(|error| {
        DimensionResolutionError::InvalidDefinition {
            reason: error.to_string(),
        }
    })?;
    let mut ids = std::collections::BTreeSet::new();
    walk(&value, &mut ids);
    Ok(ids.into_iter().collect())
}
