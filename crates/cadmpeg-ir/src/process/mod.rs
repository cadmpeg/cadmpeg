// SPDX-License-Identifier: Apache-2.0
//! CADIR v11 process types: manufacturing definitions and their evaluations.
//!
//! Setups, programs, operations and modifiers, machining features,
//! technology, tools, presets, machines, process groups, toolpath motions,
//! and process states. A process record refers to the design through
//! selections, constructions, design parameters, and design states used as
//! bases. No design record refers to a process record.
//!
//! Each type has the wire form of §2.5 of the CADIR v11 specification. Read a
//! document that holds them through [`crate::design::read::from_str`], which
//! reads a document as deep as the bounds of §2.5 admit and walks each chain
//! of state bases under [`cadmpeg_core::decode::ResourceLimits`].

pub mod cycle;
pub mod feature;
pub mod groups;
pub mod limits;
pub mod operation;
pub mod settings;
pub mod setup;
pub mod state;
pub mod stock;
pub mod technology;
pub mod tool;
pub mod toolpath;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{Construction, ConstructionKind};
use crate::design::dimension::{Dimension, ParamRef};
use crate::design::ids::ProcessParameterId;
use crate::design::selection::OrderedSelection;

/// A dimension of a process value, which can bind a process or a design
/// parameter.
pub type ProcessDimension<T> = Dimension<T, ParamRef>;

/// A selection among variants: a stock rule, a WCS mode, a height reference,
/// a coolant mode.
///
/// Where the source holds the choice in parameters, `parameters` names every
/// parameter the variant is read from, and `value` is the decoder's reading
/// of them together: a cache, absent where one of their values is absent.
/// Where `parameters` is empty the source holds the choice as a constant, and
/// `value` is present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    deny_unknown_fields,
    bound(
        serialize = "T: Serialize",
        deserialize = "T: ::serde::de::DeserializeOwned"
    )
)]
pub struct Choice<T> {
    /// The variant the parameters map to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_value"
    )]
    pub value: Option<T>,
    /// The parameters the variant is read from. The variant's own fields bind
    /// their own parameters, and this set does not name them.
    #[serde(with = "cadmpeg_core::distinct_keys::set")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<ProcessParameterId>"))]
    pub parameters: BTreeSet<ProcessParameterId>,
}

/// The selection of one workpiece's entities by a process record.
///
/// A process record that selects workpiece geometry holds one per workpiece,
/// in source order. Its picks name design entities, with witnesses in design
/// coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WorkpieceSelection {
    /// An index into the `workpieces` of the operation's setup. `None` is a
    /// selection of design entities that no workpiece places, read in the
    /// definition's frame; a record holds at most one such entry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_workpiece"
    )]
    pub workpiece: Option<u32>,
    /// The selection of the workpiece's entities.
    pub selection: OrderedSelection,
}

/// A construction in a process record that picks workpiece geometry.
///
/// Its construction's picks name design entities of the workpiece, and its
/// `resolved` value is in setup space: the picks' geometry under the
/// workpiece's `placement`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "K: JsonSchema, K::Value: JsonSchema")
)]
#[serde(deny_unknown_fields, bound(serialize = "K: Clone", deserialize = ""))]
pub struct ProcessConstruction<K: ConstructionKind> {
    /// An index into the `workpieces` of the operation's setup. `None` reads
    /// design entities in the definition's frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_workpiece"
    )]
    pub workpiece: Option<u32>,
    /// The construction.
    pub construction: Construction<K>,
}

cadmpeg_core::named_optional_field!(deserialize_value<T>, T, "value");
cadmpeg_core::named_optional_field!(deserialize_workpiece, u32, "workpiece");
