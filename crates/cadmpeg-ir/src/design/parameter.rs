// SPDX-License-Identifier: Apache-2.0
//! Design parameters: the named values of a scope.
//!
//! A parameter's expression states how its value follows from literals,
//! other parameters, and measurements of the geometry. Its value is the cache
//! of that definition, in canonical units.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::expr::Expression;
use super::ids::{ContainerId, ParameterProgramId};
use super::options::NativeOptions;
use super::units::{Unit, ValueKind};
use crate::features::{FeatureId, ParameterId};
use crate::ids::{AttributeId, ProductDefinitionId};
use crate::pmi::LimitsAndFits;
use crate::scalar::FiniteReal;

/// A named value of a scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DesignParameter {
    /// The parameter's identity.
    pub id: ParameterId,
    /// The definition whose design holds the parameter.
    pub scope: ProductDefinitionId,
    /// The record the source lists the parameter under. `None` is a user
    /// parameter of the scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner"
    )]
    pub owner: Option<ParameterOwner>,
    /// The source's name without its owner's name. `None` only for a
    /// spreadsheet cell with no alias.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<NonBlankString>,
    /// The declared quantity.
    pub kind: ValueKind,
    /// The unit the source declares for the parameter. It occurs only with a
    /// `Quantity` kind and has that quantity's exponents.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unit"
    )]
    pub unit: Option<Unit>,
    /// How the value is determined.
    pub definition: ParameterDefinition,
    /// The cache of the definition in canonical units, as the source stores
    /// it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_value"
    )]
    pub value: Option<ParameterValue>,
    /// The source's diameter or radius presentation of the value, only where
    /// it differs from what the dimension that binds the parameter measures.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_display"
    )]
    pub display: Option<DimensionDisplay>,
    /// The tolerance of the dimension that the parameter's value drives, in
    /// the parameter's canonical unit. `None` is a tolerance not stated or not
    /// read.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tolerance"
    )]
    pub tolerance: Option<Tolerance>,
    /// The key by which a definition's general tolerance by decimal places
    /// looks up the dimension's entry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_decimals"
    )]
    pub decimals: Option<u8>,
    /// The source's comment on the parameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_comment"
    )]
    pub comment: Option<String>,
    /// Source facts whose meaning is not established, and typed facts whose
    /// carrier is unread.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source facts with no design meaning.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
    /// The record that gives the parameter its identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The record the source lists a parameter under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ParameterOwner {
    /// A model parameter of a feature.
    Feature(FeatureId),
    /// A property of an ordered container that an expression binds.
    Container(ContainerId),
    /// A parameter minted for a source property linked to a dimension or to a
    /// measure, owned by the model attribute that holds the property.
    Attribute(AttributeId),
}

/// How a parameter's value is determined.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ParameterDefinition {
    /// By an expression; a literal parameter holds a one-literal expression.
    Expression(Expression),
    /// Geometry determines the value: a reference dimension.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Driven,
    /// A parameter program sets the value.
    Assigned(ParameterProgramId),
}

/// A parameter's cached value; the variant follows the parameter's kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ParameterValue {
    /// A quantity, in its canonical unit.
    Number(FiniteReal),
    /// An integer.
    Integer(i64),
    /// A Boolean.
    Boolean(bool),
    /// A text.
    Text(String),
}

/// A source's diameter or radius presentation of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DimensionDisplay {
    /// As a diameter.
    Diameter,
    /// As a radius.
    Radius,
}

/// The tolerance of a dimension: the document's one tolerance type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Tolerance {
    /// The limits less the nominal, in the canonical unit of the dimension's
    /// quantity.
    Deviations(Deviations),
    /// A limits-and-fits designation of the feature the dimension sizes, with
    /// the mating feature's, and the deviations the source stores beside it.
    Fit {
        /// The designation of the feature the dimension sizes.
        fit: LimitsAndFits,
        /// The mating feature's designation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_mate"
        )]
        mate: Option<LimitsAndFits>,
        /// The deviations the source stores beside the designation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_deviations"
        )]
        deviations: Option<FitDeviations>,
    },
    /// The dimension has no tolerance of its own; the definition's general
    /// tolerance applies.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    General,
    /// The dimension is theoretically exact: it has no tolerance of its own
    /// and the general tolerance does not apply to it.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Basic,
    /// One limit only: the nominal is that limit, and the value may lie
    /// anywhere beyond it on the other side.
    SingleLimit(LimitSide),
}

/// Two deviations, each the limit less the nominal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Deviations {
    /// The lower deviation.
    pub lower: FiniteReal,
    /// The upper deviation.
    pub upper: FiniteReal,
}

impl Deviations {
    /// The deviations of the limits `lower` and `upper` about `nominal`.
    #[must_use]
    pub fn from_limits(nominal: f64, lower: f64, upper: f64) -> Option<Self> {
        Some(Self {
            lower: FiniteReal::new(lower - nominal)?,
            upper: FiniteReal::new(upper - nominal)?,
        })
    }
}

/// A fit's stored deviations: the designation's published cell (a cache),
/// the source's own where they differ from it, or deviations no table the
/// decoder holds checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FitDeviations {
    /// The designation's cell at the nominal in a published table the decoder
    /// holds.
    Cell(Deviations),
    /// The source's own deviations, which differ from that cell.
    Own(Deviations),
    /// Deviations of a designation whose standard's table the decoder does
    /// not hold.
    Unchecked(Deviations),
}

/// The side of a single-limit dimension's limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LimitSide {
    /// The nominal is the least permitted value.
    Minimum,
    /// The nominal is the greatest permitted value.
    Maximum,
}

cadmpeg_core::named_optional_field!(deserialize_comment<T>, T, "comment");
cadmpeg_core::named_optional_field!(deserialize_decimals<T>, T, "decimals");
cadmpeg_core::named_optional_field!(deserialize_deviations<T>, T, "deviations");
cadmpeg_core::named_optional_field!(deserialize_display<T>, T, "display");
cadmpeg_core::named_optional_field!(deserialize_mate<T>, T, "mate");
cadmpeg_core::named_optional_field!(deserialize_name<T>, T, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_owner<T>, T, "owner");
cadmpeg_core::named_optional_field!(deserialize_tolerance<T>, T, "tolerance");
cadmpeg_core::named_optional_field!(deserialize_unit<T>, T, "unit");
cadmpeg_core::named_optional_field!(deserialize_value<T>, T, "value");
