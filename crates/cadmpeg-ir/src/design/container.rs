// SPDX-License-Identifier: Apache-2.0
//! Containers: the ordered histories and unordered sets that hold features.
//!
//! Membership lives only in containers. Every feature and container is a
//! member of exactly one container or is a root of exactly one scope;
//! containers form a tree.

use std::collections::BTreeSet;

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::construction::{kinds, Construction};
use super::ids::ContainerId;
use super::refs::Member;

/// A container of features: an ordered history that builds one cumulative
/// state, or an unordered set whose members each have their own result.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Container {
    /// An ordered container, which builds one cumulative state.
    Ordered {
        /// The container's identity.
        id: ContainerId,
        /// What the ordered container is.
        kind: OrderedKind,
        /// The members, in order.
        members: Vec<Member>,
        /// Display groupings of contiguous runs of the members.
        folders: Vec<Folder>,
        /// How far the source builds the container. `None` where no stored
        /// fact the decoder read fixes it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rollback"
        )]
        rollback: Option<Rollback>,
        /// The placement the source applies to the whole of the container's
        /// result, as a construction whose components can be bound. `None`
        /// where the source has none.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placement"
        )]
        placement: Option<Box<Construction<kinds::Frame>>>,
    },
    /// An unordered container, whose members each have their own result. It
    /// has no rollback and no order.
    Unordered {
        /// The container's identity.
        id: ContainerId,
        /// What the unordered container is.
        kind: UnorderedKind,
        /// The members.
        #[serde(with = "cadmpeg_core::distinct_keys::set")]
        #[cfg_attr(feature = "schema", schemars(with = "BTreeSet<Member>"))]
        members: BTreeSet<Member>,
    },
}

impl Container {
    /// The container's identity.
    #[must_use]
    pub const fn id(&self) -> &ContainerId {
        match self {
            Self::Ordered { id, .. } | Self::Unordered { id, .. } => id,
        }
    }
}

/// What an ordered container is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OrderedKind {
    /// A history of any number of bodies.
    Timeline,
    /// An ordered container whose solid features build one body and act on
    /// it (a `FreeCAD` `PartDesign` body, a CATIA body).
    Body,
    /// A CATIA ordered geometrical set: wireframe and surface features in
    /// order, each result a body of its own.
    GeometricalSet,
}

/// What an unordered container is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UnorderedKind {
    /// A CATIA geometrical set.
    GeometricalSet,
    /// A group of independent members.
    Group,
}

/// A display grouping of a contiguous run of an ordered container's members,
/// `first` to `last` by index. It scopes no state, has no position, and has
/// no rollback. Folders of one container nest or are disjoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Folder {
    /// The folder's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The index of the first member of the run.
    pub first: u32,
    /// The index of the last member of the run.
    pub last: u32,
}

/// An ordered container's rollback position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Rollback {
    /// After the last member.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    End,
    /// Before a member of the container.
    Before(Member),
}

cadmpeg_core::named_optional_field!(deserialize_name<T>, T, "name");
cadmpeg_core::named_optional_field!(deserialize_placement<T>, T, "placement");
cadmpeg_core::named_optional_field!(deserialize_rollback<T>, T, "rollback");
