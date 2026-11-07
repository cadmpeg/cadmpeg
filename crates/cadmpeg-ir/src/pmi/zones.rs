// SPDX-License-Identifier: Apache-2.0
//! Geometric tolerance-zone constraints in canonical millimetres.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::PmiTarget;
use crate::scalar::{Length, PositiveLength};

/// Additional size and placement constraints on a geometric tolerance.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ToleranceConstraints {
    /// Signed displacement of an unequally disposed tolerance zone.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_displacement"
    )]
    pub displacement: Option<Length>,
    /// Maximum permitted upper tolerance in millimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_maximum"
    )]
    pub maximum: Option<PositiveLength>,
    /// Independently specified tolerance zones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<ToleranceZone>,
}

impl ToleranceConstraints {
    /// Whether the source specifies no additional constraint.
    pub fn is_empty(&self) -> bool {
        self.displacement.is_none() && self.maximum.is_none() && self.zones.is_empty()
    }
}

/// Shape and extent of one geometric tolerance zone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ToleranceZone {
    /// Source-defined zone form, such as `cylindrical or circular`.
    pub form: NonBlankString,
    /// Projected extents belonging to this zone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projections: Vec<ToleranceProjection>,
}

/// A projected tolerance zone and its terminating feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ToleranceProjection {
    /// Features that bound the projected zone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boundaries: Vec<PmiTarget>,
    /// Feature at the projection end.
    pub end: PmiTarget,
    /// Positive projection length in millimetres.
    pub length: PositiveLength,
    /// Signed projection offset in millimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_offset"
    )]
    pub offset: Option<Length>,
}

rewrite_record!(ToleranceConstraints, []; {displacement, maximum, zones});
rewrite_record!(ToleranceZone, []; {form, projections});
rewrite_record!(ToleranceProjection, []; {boundaries, end, length, offset});

cadmpeg_core::named_optional_field!(deserialize_displacement, Length, "displacement");
cadmpeg_core::named_optional_field!(deserialize_maximum, PositiveLength, "maximum");
cadmpeg_core::named_optional_field!(deserialize_offset, Length, "offset");

impl ToleranceProjection {
    /// Features that bound or terminate this projection.
    pub fn targets(&self) -> impl Iterator<Item = &PmiTarget> {
        self.boundaries.iter().chain(std::iter::once(&self.end))
    }
}

#[cfg(test)]
mod tests {
    use super::ToleranceConstraints;

    #[test]
    fn constraints_refuse_null_nonpositive_and_unknown_members() {
        for value in [
            serde_json::json!({"maximum": null}),
            serde_json::json!({"maximum": 0.0}),
            serde_json::json!({"maximum": -1.0}),
            serde_json::json!({"displacement": null}),
            serde_json::json!({"unrecognized": true}),
        ] {
            assert!(serde_json::from_value::<ToleranceConstraints>(value).is_err());
        }
    }
}
