// SPDX-License-Identifier: Apache-2.0
//! Types that more than one operation family shares.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::expr::Expression;
use crate::design::options::{LimitOffset, SymmetricMeasure};
use crate::design::selection::{OrderedSelection, SetSelection};
use crate::features::ParameterId;

/// A size measured on the radius or on the diameter, as the source states
/// it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(
        serialize = "Q: Serialize, P: Serialize",
        deserialize = "Q: Deserialize<'de>, P: Deserialize<'de>"
    )
)]
pub enum RadialDimension<Q, P = ParameterId> {
    /// The size on the radius.
    Radius(Dimension<Q, P>),
    /// The size on the diameter.
    Diameter(Dimension<Q, P>),
}

/// Material placed along a direction or about an axis, measured from the
/// operation's origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Span<Q> {
    /// From `start` to `end`. `end` is measured along the direction and
    /// `start` against it.
    Between {
        /// The limit against the direction.
        start: Box<Limit<Q>>,
        /// The limit along the direction.
        end: Box<Limit<Q>>,
    },
    /// Both sides of the origin equally.
    Symmetric {
        /// The amount.
        amount: Dimension<Q>,
        /// Whether `amount` is the extent of each side or the total.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_measure"
        )]
        measure: Option<SymmetricMeasure>,
    },
}

/// One end of a span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Limit<Q> {
    /// At the operation's origin.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    AtOrigin,
    /// A signed distance or angle from the origin.
    Amount(Dimension<Q>),
    /// Through every target body; in a revolve, a full turn.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ThroughAll,
    /// At the first face the swept profile meets.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Next,
    /// At the last face the swept profile meets.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Last,
    /// At faces, surfaces, or bodies.
    ToGeometry {
        /// The faces, surfaces, or bodies.
        target: SetSelection,
        /// The offset of the limit from the target.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<LimitOffset>,
        /// Whether the target faces extend until the profile meets them.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extend"
        )]
        extend: Option<Dimension<bool>>,
        /// Where `target` picks bodies, whether the limit is the far side of
        /// the body the profile meets.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_far"
        )]
        far: Option<Dimension<bool>>,
    },
    /// At a plane.
    ToPlane(Construction<kinds::Plane>),
    /// Where the swept profile reaches a point.
    ToPoint(Construction<kinds::Point>),
    /// A source limit kind whose meaning is not established.
    Native(NonBlankString),
}

/// A function of a quantity along a path parameter from 0 to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(serialize = "Q: Serialize", deserialize = "Q: Deserialize<'de>")
)]
pub enum Law<Q> {
    /// A constant.
    Constant(Dimension<Q>),
    /// A linear ramp.
    Linear {
        /// The value at the path's start.
        start: Dimension<Q>,
        /// The value at the path's end.
        end: Dimension<Q>,
    },
    /// A law curve that the source selects.
    Curve(OrderedSelection),
    /// An expression in the law parameter whose result has the quantity.
    Expression(Expression),
    /// A source law whose meaning is not established.
    Native(NonBlankString),
}

/// How a value or a section changes between consecutive stations or
/// sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Interpolation {
    /// Each consecutive pair blends linearly, which leaves an edge at each
    /// intermediate section.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Linear,
    /// One smooth surface passes through every section.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Smooth,
    /// A source interpolation whose meaning is not established.
    Native(NonBlankString),
}

/// The corner law between offset faces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OffsetJoin {
    /// Rounded arcs.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Round,
    /// Sharp intersection: the sides extend to meet.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Sharp,
    /// A source join whose meaning is not established.
    Native(NonBlankString),
}

/// The faces of a sheet-metal wall that a cut's side faces are normal to, the
/// cut being made as if the part were flat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SheetCutNormal {
    /// The face the wall is built from.
    Defining,
    /// The other face.
    Opposite,
    /// A cut through the thickness in the flat state.
    Both,
}

cadmpeg_core::named_optional_field!(deserialize_extend, Dimension<bool>, "extend");
cadmpeg_core::named_optional_field!(deserialize_far, Dimension<bool>, "far");
cadmpeg_core::named_optional_field!(deserialize_measure, SymmetricMeasure, "measure");
cadmpeg_core::named_optional_field!(deserialize_offset, LimitOffset, "offset");

#[cfg(test)]
mod tests;
