// SPDX-License-Identifier: Apache-2.0
//! Design scopes: the design of one product definition.
//!
//! A design scope is the history, parameters, configurations, and current
//! states of one product definition. A document holds at most one scope per
//! definition.

use std::collections::{BTreeMap, BTreeSet};

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::configuration::{ArrangementRef, ParameterOverride};
use super::evaluation::CurrentState;
use super::ids::{ArrangementId, ContainerId};
use super::refs::ConfigurationRef;
use crate::features::{ConfigurationId, ParameterId};
use crate::ids::{OccurrenceId, ProductDefinitionId};

/// The design of one product definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DesignScope {
    /// The product definition whose design this is.
    pub definition: ProductDefinitionId,
    /// The scope's root containers.
    pub roots: Vec<ContainerId>,
    /// The configuration whose state the current geometry holds for the
    /// scope. `None` when the scope has no configurations.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_active_configuration"
    )]
    pub active_configuration: Option<ConfigurationId>,
    /// The arrangements in force in the scope's base, at most one of each
    /// kind.
    #[serde(with = "cadmpeg_core::distinct_keys::set")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<ArrangementId>"))]
    pub active_arrangements: BTreeSet<ArrangementId>,
    /// The design state of each occurrence owned by a definition the scope
    /// spans, as base values.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub occurrences: BTreeMap<OccurrenceId, OccurrenceDesign>,
    /// The scope's current states.
    pub current: Vec<CurrentState>,
}

/// The base design state of one occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OccurrenceDesign {
    /// The configuration of its prototype that the occurrence uses. With none
    /// the occurrence uses its prototype's active configuration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<ConfigurationRef>,
    /// The arrangements of its prototype that the occurrence uses, at most
    /// one of each kind.
    pub arrangements: Vec<ArrangementRef>,
    /// Whether the source fixes the occurrence in place.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_grounded"
    )]
    pub grounded: Option<bool>,
    /// Whether the parent solves the occurrence's nested placements from the
    /// sub-assembly's own joints.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flexible"
    )]
    pub flexible: Option<bool>,
    /// Values of the prototype's parameters in this occurrence alone, keyed
    /// by parameter of the prototype's scope.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub parameters: BTreeMap<ParameterId, ParameterOverride>,
}

cadmpeg_core::named_optional_field!(
    deserialize_active_configuration<T>,
    T,
    "active_configuration"
);
cadmpeg_core::named_optional_field!(deserialize_configuration<T>, T, "configuration");
cadmpeg_core::named_optional_field!(deserialize_flexible<T>, T, "flexible");
cadmpeg_core::named_optional_field!(deserialize_grounded<T>, T, "grounded");
