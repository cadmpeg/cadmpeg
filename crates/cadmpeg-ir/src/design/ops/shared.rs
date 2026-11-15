// SPDX-License-Identifier: Apache-2.0
//! Types that more than one family of the feature vocabulary uses.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::dimension::Dimension;
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
