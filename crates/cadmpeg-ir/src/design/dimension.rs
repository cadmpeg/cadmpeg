// SPDX-License-Identifier: Apache-2.0
//! Dimensions: values that a parameter can drive, and references to
//! parameters.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::ProcessParameterId;
use super::selection::OccurrencePath;
use crate::features::ParameterId;
use crate::ids::ProductDefinitionId;

/// A value that a parameter can drive: a literal, or a binding to a
/// parameter. A bound dimension stores no value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(
        serialize = "T: Serialize, P: Serialize",
        deserialize = "T: Deserialize<'de>, P: Deserialize<'de>"
    )
)]
pub enum Dimension<T, P = ParameterId> {
    /// The value itself.
    Literal(T),
    /// A binding to a parameter of the scope that holds the dimension.
    Parameter(P),
    /// A binding to a parameter with the opposite sign, or the complement
    /// where `T` is `bool`. It occurs only where `T` is a signed quantity or
    /// `bool`.
    Negated(P),
}

/// A reference to a design parameter of any scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ParameterRef {
    /// The parameter.
    pub target: RefTarget,
    /// The occurrence path, from the referencing scope's definition, through
    /// which the source names the parameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_through"
    )]
    pub through: Option<OccurrencePath>,
}

/// The parameter that a [`ParameterRef`] names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RefTarget {
    /// A parameter of this document.
    Parameter(ParameterId),
    /// A parameter, by its source name, of a definition with `External`
    /// storage whose document was not read.
    External {
        /// The definition that holds the parameter.
        definition: ProductDefinitionId,
        /// The source name of the parameter.
        name: NonBlankString,
    },
}

/// A reference from a process expression or a process dimension: a process
/// parameter or a design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ParamRef {
    /// A process parameter.
    Process(ProcessParameterId),
    /// A design parameter.
    Design(ParameterRef),
}

cadmpeg_core::named_optional_field!(deserialize_through, OccurrencePath, "through");
