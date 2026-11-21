// SPDX-License-Identifier: Apache-2.0
//! Datums: reference geometry.
//!
//! A datum feature holds points, an axis, a plane, or a coordinate frame,
//! each built by a construction. A datum has no topology; its value is its
//! construction's `resolved`.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::construction::{kinds, Construction};
use crate::design::feature::Link;
use crate::design::options::NativeOptions;

/// A datum feature: reference geometry built by a construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Datum {
    /// The reference geometry.
    pub element: DatumElement,
    /// The rest of the link of a datum copied from another datum, whose
    /// construction picks the source datum.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_link"
    )]
    pub link: Option<Link>,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source settings that size the datum's display.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// The reference geometry of a datum.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DatumElement {
    /// One or more points, each picked by index. Never empty.
    Points(Vec<Construction<kinds::Point>>),
    /// An axis.
    Axis(Construction<kinds::Axis>),
    /// A plane.
    Plane(Construction<kinds::Plane>),
    /// A coordinate frame.
    Frame(Construction<kinds::Frame>),
}

cadmpeg_core::named_optional_field!(deserialize_link<T>, T, "link");
