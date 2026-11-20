// SPDX-License-Identifier: Apache-2.0
//! A definition's rule for every dimension that states no tolerance of its
//! own (CADIR v11 §8.1).

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::design::parameter::Deviations;

/// The tolerance of a dimension that states `Tolerance::General`: one rule
/// in force for lengths and one for angles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GeneralTolerance {
    /// The rule for lengths, radii, and diameters.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_linear"
    )]
    pub linear: Option<LinearGeneral>,
    /// The rule for angles.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angular"
    )]
    pub angular: Option<AngularGeneral>,
}

/// The general rule for lengths, radii, and diameters.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LinearGeneral {
    /// ISO 2768-1 table 1 applied to every length, radius, and diameter: the
    /// symmetric deviation by the class and the nominal size's range.
    Iso2768(LinearGeneralClass),
    /// The deviations by the dimension's `decimals`, the key the rule looks
    /// a dimension up by.
    ByDecimals(
        #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
        #[cfg_attr(feature = "schema", schemars(with = "Vec<(u8, Deviations)>"))]
        BTreeMap<u8, Deviations>,
    ),
    /// One pair of deviations for every length, radius, and diameter.
    Uniform(Deviations),
}

/// The general rule for angles.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AngularGeneral {
    /// The deviations by the dimension's `decimals`, the key the rule looks
    /// a dimension up by.
    ByDecimals(
        #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
        #[cfg_attr(feature = "schema", schemars(with = "Vec<(u8, Deviations)>"))]
        BTreeMap<u8, Deviations>,
    ),
    /// One pair of deviations for every angle.
    Uniform(Deviations),
}

/// An ISO 2768-1 tolerance class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LinearGeneralClass {
    /// Class f.
    Fine,
    /// Class m.
    Medium,
    /// Class c.
    Coarse,
    /// Class v.
    VeryCoarse,
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_linear, LinearGeneral, "linear");
cadmpeg_core::named_optional_field!(deserialize_angular, AngularGeneral, "angular");
