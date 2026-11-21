// SPDX-License-Identifier: Apache-2.0
//! Design parameters and configurations.

use super::selections::DistinctMembers;
use super::{
    deserialize_dependencies, deserialize_parameter_real, ConfigurationId, FeatureDefinition,
    FeatureId, ParameterId,
};
use crate::ids::{BodyId, ProductDefinitionId};
use crate::scalar::{Angle, FiniteReal, Length};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

cadmpeg_core::named_optional_field!(deserialize_bodies, DistinctMembers<BodyId>, "bodies");

cadmpeg_core::named_optional_field!(deserialize_display, DimensionDisplay, "display");

cadmpeg_core::named_optional_field!(deserialize_display_text, String, "display_text");

cadmpeg_core::named_optional_field!(deserialize_material, String, "material");

cadmpeg_core::named_optional_field!(deserialize_name, String, "name");

cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

cadmpeg_core::named_optional_field!(deserialize_owner, FeatureId, "owner");

cadmpeg_core::named_optional_field!(deserialize_pmi, ParameterPmi, "pmi");

cadmpeg_core::named_optional_field!(
    deserialize_product_definition,
    ProductDefinitionId,
    "product_definition"
);

cadmpeg_core::named_optional_field!(deserialize_source_index, u32, "source_index");

cadmpeg_core::named_optional_field!(deserialize_value, ParameterValue, "value");

/// A named parametric model variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DesignConfiguration {
    /// Globally unique configuration id.
    pub id: ConfigurationId,
    /// Position in the design configuration list.
    pub ordinal: u32,
    /// Whether this configuration supplies the document's active model state.
    pub active: bool,
    /// Format-native configuration slot, when distinct from list order.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_index"
    )]
    pub source_index: Option<u32>,
    /// Source display name, when established.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Material override, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_material"
    )]
    pub material: Option<String>,
    /// Configuration-local named values not otherwise represented.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub properties: BTreeMap<NonBlankString, String>,
    /// Configuration-specific source expressions keyed by the overridden parameter.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameter_overrides: BTreeMap<ParameterId, String>,
    /// Bodies present when this configuration is active.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bodies"
    )]
    pub bodies: Option<DistinctMembers<BodyId>>,
    /// Evaluated parameter state when this configuration is active.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameter_values: BTreeMap<ParameterId, ParameterValue>,
    /// Evaluated feature operation state when this configuration is active.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub feature_states: BTreeMap<FeatureId, ConfigurationFeatureState>,
    /// Identifier of the full-fidelity record in a native namespace.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

impl DesignConfiguration {
    /// Features suppressed when this configuration is active.
    pub fn suppressed_features(&self) -> impl Iterator<Item = &FeatureId> {
        self.feature_states
            .iter()
            .filter_map(|(feature, state)| state.evaluation.is_suppressed().then_some(feature))
    }
}

/// Configuration-local evaluation state for one construction feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ConfigurationFeatureState {
    /// Whether evaluation produced bodies or was suppressed.
    pub evaluation: ConfigurationEvaluation,
    /// Earlier features consumed during regeneration in source operand order.
    #[serde(deserialize_with = "deserialize_dependencies")]
    pub dependencies: DistinctMembers<FeatureId>,
    /// Evaluated construction semantics in the configuration.
    pub definition: FeatureDefinition,
}

/// Result of evaluating one feature in a configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigurationEvaluation {
    /// The feature is suppressed for this configuration.
    Suppressed {},
    /// The feature is active; the output list may be empty for operations
    /// whose neutral result is carried by the surrounding topology.
    Active {
        /// Bodies produced or modified in the configuration.
        outputs: DistinctMembers<BodyId>,
    },
}

impl<'de> Deserialize<'de> for ConfigurationEvaluation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(cadmpeg_derive::DeserializeTagged)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Suppressed {},
            Active { outputs: Vec<BodyId> },
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Suppressed {} => Self::Suppressed {},
            Wire::Active { outputs } => {
                let mut seen = std::collections::HashSet::new();
                seen.try_reserve(outputs.len())
                    .map_err(serde::de::Error::custom)?;
                for output in &outputs {
                    if !seen.insert(output) {
                        return Err(serde::de::Error::custom(
                            "outputs: members must be distinct",
                        ));
                    }
                }
                Self::Active {
                    outputs: DistinctMembers(outputs),
                }
            }
        })
    }
}

impl ConfigurationEvaluation {
    /// Whether evaluation of the feature is suppressed.
    #[must_use]
    pub const fn is_suppressed(&self) -> bool {
        matches!(self, Self::Suppressed {})
    }

    /// Bodies produced or modified by an active feature.
    #[must_use]
    pub fn outputs(&self) -> &[BodyId] {
        match self {
            Self::Suppressed {} => &[],
            Self::Active { outputs } => outputs,
        }
    }
}

/// A named design expression, optionally owned by a construction feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DesignParameter {
    /// Product definition whose frame contains this parameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_product_definition"
    )]
    pub product_definition: Option<ProductDefinitionId>,
    /// Globally unique parameter id.
    pub id: ParameterId,
    /// Feature that consumes this parameter; absent for a document parameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_owner"
    )]
    pub owner: Option<FeatureId>,
    /// Position among parameters in the same ownership scope.
    pub ordinal: u32,
    /// Source parameter name.
    pub name: String,
    /// Literal or expression text used by the source system.
    pub expression: String,
    /// Geometric display semantics carried by the dimension expression.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_display"
    )]
    pub display: Option<DimensionDisplay>,
    /// Evaluated scalar when available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_value"
    )]
    pub value: Option<ParameterValue>,
    /// Parameters referenced by `expression`, in source expression order.
    #[serde(deserialize_with = "deserialize_dependencies")]
    pub dependencies: DistinctMembers<ParameterId>,
    /// Source parameter properties not represented by another field.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub properties: BTreeMap<NonBlankString, String>,
    /// Product-manufacturing dimension semantics, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pmi"
    )]
    pub pmi: Option<ParameterPmi>,
    /// Identifier of the full-fidelity source parameter record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// Product-manufacturing semantics attached to a design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ParameterPmi {
    /// Semantic dimension family.
    pub subtype: PmiDimensionSubtype,
    /// Display precision carried by the semantic annotation.
    pub precision: i64,
    /// Native formatted dimension text.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_display_text"
    )]
    pub display_text: Option<String>,
    /// Basic-dimension flag.
    pub basic: bool,
    /// Inspection-dimension flag.
    pub inspection: bool,
    /// Reference-only flag.
    pub reference_only: bool,
    /// Identifier of the full-fidelity semantic record.
    pub native_ref: String,
}

/// Semantic PMI dimension family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PmiDimensionSubtype {
    /// Linear distance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Linear,
    /// Angular extent in radians.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Angle,
    /// Diameter.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Diameter,
    /// Radius.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Radial,
    /// Linear coordinate measured from an ordinate origin.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Ordinate,
    /// Dimensionless integral instance count.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Count,
    /// Source-native family without a neutral equivalent.
    Native(String),
}

/// Geometric interpretation requested by a dimension display modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DimensionDisplay {
    /// Displays the dimension as a diameter.
    Diameter,
    /// Displays the dimension as a radius.
    Radius,
}

/// Canonical scalar value of a literal design parameter.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ParameterValue {
    /// Length in canonical millimeters.
    Length(Length),
    /// Angle in canonical radians.
    Angle(Angle),
    /// Dimensionless real scalar.
    Real(#[serde(deserialize_with = "deserialize_parameter_real")] FiniteReal),
    /// Integer scalar.
    Integer(i64),
    /// Boolean scalar.
    Boolean(bool),
    /// Literal text value.
    String(String),
}

/// A polygon side count of at least three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct PolygonSideCount(pub(super) u32);

impl PolygonSideCount {
    /// Admits a side count of at least three.
    pub fn new(value: u32) -> Option<Self> {
        (value >= 3).then_some(Self(value))
    }
}

impl<'de> Deserialize<'de> for PolygonSideCount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u32::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("polygon sides must be at least three"))
    }
}
