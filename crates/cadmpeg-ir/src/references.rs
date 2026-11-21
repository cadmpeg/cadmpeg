// SPDX-License-Identifier: Apache-2.0
//! Instance targets, and the references of drawings and annotations.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::names::NativeName;
use crate::design::refs::{EntityRef, StateRef};
use crate::design::selection::OccurrencePath;
use crate::features::FeatureId;
use crate::ids::{OccurrenceId, ProductDefinitionId};

/// A model-class target that can name an instance: an entity, the placed
/// instance whose prototype holds it, and the state whose geometry holds it.
///
/// A whole placed instance is its last occurrence as `entity` with the steps
/// above it as `path`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InstanceTarget<E> {
    /// The targeted entity.
    pub entity: E,
    /// The placed instance whose prototype holds `entity`, outermost
    /// occurrence first. `None` targets the entity in the definition that
    /// owns it, and so in every placement of that definition.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_path"
    )]
    pub path: Option<OccurrencePath>,
    /// The state whose geometry holds `entity`, in the scope of the
    /// definition that holds it. `None` is the current geometry, or, with a
    /// `path`, the geometry of the configuration that instance selects.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_state"
    )]
    pub state: Option<StateRef>,
}

impl<E> InstanceTarget<E> {
    /// A target on `entity` in the definition that owns it, in the current
    /// geometry.
    #[must_use]
    pub const fn new(entity: E) -> Self {
        Self {
            entity,
            path: None,
            state: None,
        }
    }
}

impl<E> InstanceTarget<E> {
    /// Whether the target references a record of the design class: a state,
    /// or a configuration of this document in a path step.
    pub(crate) fn reads_design(&self) -> bool {
        self.state.is_some()
            || self.path.as_ref().is_some_and(|path| {
                path.0.iter().any(|step| {
                    matches!(
                        step.configuration,
                        Some(crate::design::refs::ConfigurationRef::Local(_))
                    )
                })
            })
    }
}

impl<E> From<E> for InstanceTarget<E> {
    fn from(entity: E) -> Self {
        Self::new(entity)
    }
}

/// A drawing relationship or an annotation reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ReferenceSelection {
    /// The referenced object.
    pub target: ReferenceTarget,
    /// The source's object token and sub-element string, where the target
    /// is `Unresolved`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native"
    )]
    pub native: Option<NativeName>,
}

impl ReferenceSelection {
    /// A reference to `target` with no native name.
    #[must_use]
    pub const fn new(target: ReferenceTarget) -> Self {
        Self {
            target,
            native: None,
        }
    }

    /// A reference that resolves to no object the document holds, by the
    /// source's name for it.
    #[must_use]
    pub const fn unresolved(native: NativeName) -> Self {
        Self {
            target: ReferenceTarget::Unresolved,
            native: Some(native),
        }
    }

    /// The source's text name for the referenced object, where it states
    /// one.
    #[must_use]
    pub fn native_text(&self) -> Option<&str> {
        match &self.native.as_ref()?.value {
            crate::design::names::NativeNameValue::Text(text) => Some(text.as_str()),
            crate::design::names::NativeNameValue::Integers(_)
            | crate::design::names::NativeNameValue::Bytes(_) => None,
        }
    }
}

/// The object of a drawing or annotation reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ReferenceTarget {
    /// The source's explicit empty reference.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Null,
    /// An entity of the geometry or the products the document holds.
    Entity(InstanceTarget<ReferenceEntity>),
    /// A feature, such as the result a drawing view shows.
    Feature(FeatureId),
    /// An object of a document that was not read.
    External {
        /// The document.
        document: String,
        /// The object's token in that document.
        object: String,
    },
    /// A target the decoder cannot resolve to an object the document holds.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unresolved,
}

impl ReferenceTarget {
    /// Whether the target references a record of the design class.
    pub(crate) fn reads_design(&self) -> bool {
        match self {
            Self::Entity(target) => target.reads_design(),
            Self::Feature(_) => true,
            Self::Null | Self::External { .. } | Self::Unresolved => false,
        }
    }

    /// Whether every record the target names is in the document: an entity
    /// by `contains`, which looks a model-class identity up, and a feature
    /// in `records`.
    pub(crate) fn resolves(
        &self,
        records: &crate::document::design_records::DesignRecords<'_>,
        contains: impl Fn(&str) -> bool,
    ) -> bool {
        match self {
            Self::Entity(target) => contains(target.entity.id()),
            Self::Feature(feature) => records.holds_feature(feature),
            Self::Null | Self::External { .. } | Self::Unresolved => true,
        }
    }
}

/// An entity that a reference names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ReferenceEntity {
    /// A body, face, edge, vertex, carrier, or cage.
    Topology(EntityRef),
    /// A product definition.
    Definition(ProductDefinitionId),
    /// An occurrence.
    Occurrence(OccurrenceId),
}

impl ReferenceEntity {
    /// The entity's record id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Topology(entity) => entity.id(),
            Self::Definition(id) => id.as_str(),
            Self::Occurrence(id) => id.as_str(),
        }
    }
}

cadmpeg_core::named_optional_field!(deserialize_path, OccurrencePath, "path");
cadmpeg_core::named_optional_field!(deserialize_state, StateRef, "state");
cadmpeg_core::named_optional_field!(deserialize_native, NativeName, "native");

#[cfg(test)]
mod tests;

mod identity_rewrite;
