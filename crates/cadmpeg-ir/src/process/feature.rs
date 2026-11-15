// SPDX-License-Identifier: Apache-2.0
//! Machining features (§7.4).
//!
//! A machining feature is a region of the part to machine, stated once and
//! shared by the operations that machine it. Its dimensions and end
//! conditions are parameters it owns.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::operation::ProcessGeometry;
use crate::design::ids::MachiningFeatureId;
use crate::design::names::NativeName;

/// A region of the part to machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MachiningFeature {
    /// The feature's identity.
    pub id: MachiningFeatureId,
    /// The source's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The feature's class.
    pub class: MachiningFeatureClass,
    /// The source's feature type, verbatim.
    pub native_kind: NativeName,
    /// The feature's geometry, by role.
    pub geometry: Vec<ProcessGeometry>,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The class of a machining feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MachiningFeatureClass {
    /// A planar face.
    PlanarFace {},
    /// A closed pocket.
    ClosedPocket {},
    /// An open pocket.
    OpenPocket {},
    /// A slot.
    Slot {},
    /// A step.
    Step {},
    /// A round hole.
    RoundHole {},
    /// A boss.
    Boss {},
    /// An outside profile.
    OutsideProfile {},
    /// A region.
    Region {},
    /// A pattern of a base feature, with its placements as parameters and
    /// geometry.
    Replicate {
        /// The patterned feature.
        base: MachiningFeatureId,
    },
    /// A join of features.
    Compound {
        /// The joined features.
        members: Vec<MachiningFeatureId>,
    },
    /// A class that no arm states; the feature's `native_kind` holds the
    /// source's name.
    Unclassified {},
}

cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
