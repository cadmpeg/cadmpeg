// SPDX-License-Identifier: Apache-2.0
//! Feature tree children, node roles and captured component positions.

use super::membership;
use super::selections::DistinctMembers;
use super::{FeatureCollectionError, FeatureId};
use crate::ids::OccurrenceId;
use crate::transform::Transform;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_active_child, FeatureId, "active_child");

cadmpeg_core::named_optional_field!(deserialize_captured_occurrence, OccurrenceId, "occurrence");

/// Distinct tree children and an optional active member.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TreeChildren {
    pub(super) children: DistinctMembers<FeatureId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) active_child: Option<FeatureId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TreeChildrenWire {
    children: Vec<FeatureId>,
    #[serde(default, deserialize_with = "deserialize_active_child")]
    active_child: Option<FeatureId>,
}

impl TreeChildren {
    /// Admit distinct children and an active member that belongs to them.
    pub fn new(
        children: Vec<FeatureId>,
        active_child: Option<FeatureId>,
    ) -> Result<Self, FeatureCollectionError> {
        Self::build(children, active_child).map_err(FeatureCollectionError::Invalid)
    }

    fn build(
        children: Vec<FeatureId>,
        active_child: Option<FeatureId>,
    ) -> Result<Self, &'static str> {
        if active_child
            .as_ref()
            .is_some_and(|active| !children.contains(active))
        {
            return Err("active_child must belong to children");
        }
        if !membership::distinct(&children, |_| true) {
            return Err("members must be distinct");
        }
        Ok(Self {
            children: DistinctMembers(children),
            active_child,
        })
    }

    /// Whether the node has no children.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Return the active child identity.
    pub fn active_child(&self) -> &Option<FeatureId> {
        &self.active_child
    }

    /// Keep only the children `keep` accepts, and no active child where
    /// `keep` refuses it.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&FeatureId) -> bool) {
        self.children.0.retain(|child| keep(child));
        if self
            .active_child
            .as_ref()
            .is_some_and(|active| !self.children.0.contains(active))
        {
            self.active_child = None;
        }
    }

    /// Add a child unless it is already a member.
    pub fn insert(&mut self, child: FeatureId) {
        self.children.insert(child);
    }
}

impl std::ops::Deref for TreeChildren {
    type Target = [FeatureId];
    fn deref(&self) -> &[FeatureId] {
        &self.children
    }
}

impl<'a> IntoIterator for &'a TreeChildren {
    type Item = &'a FeatureId;
    type IntoIter = std::slice::Iter<'a, FeatureId>;
    fn into_iter(self) -> Self::IntoIter {
        self.children.iter()
    }
}

impl<'de> Deserialize<'de> for TreeChildren {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = TreeChildrenWire::deserialize(deserializer)?;
        Self::build(wire.children, wire.active_child).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Operation mode for a local component occurrence transfer."]
pub enum ComponentTransferMode {
    #[doc = "A new local component occurrence is created from an existing occurrence."]
    Copy,
    #[doc = "An existing local component occurrence changes its containing definition."]
    Reparent,
}

#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[doc = "A canonical affine pose or the ordinal of its retained native matrix."]
pub enum CapturedPositionTransform {
    #[doc = "Finite affine pose with translation in canonical millimetres."]
    Affine {
        #[doc = "Captured pose in the occurrence assembly context."]
        transform: Transform,
    },
    #[doc = "Matrix retained in the named native scope's ordered captured rows."]
    Native {
        #[doc = "Native scope retaining the source coefficients."]
        scope: crate::ids::NativeId,
        #[doc = "Zero-based ordinal of the source matrix row."]
        row: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "One retained occurrence selector and its captured pose."]
pub struct CapturedComponentPosition {
    #[doc = "Native selector retained independently of occurrence resolution."]
    pub selector: crate::ids::NativeId,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_captured_occurrence"
    )]
    #[doc = "Expanded occurrence when the complete selector resolves uniquely."]
    pub occurrence: Option<OccurrenceId>,
    #[doc = "Captured pose in the occurrence assembly context."]
    pub transform: CapturedPositionTransform,
}

impl CapturedComponentPosition {
    #[doc = "Construct an unresolved captured pose while preserving its native selector."]
    pub fn new(selector: crate::ids::NativeId, transform: Transform) -> Self {
        Self {
            selector,
            occurrence: None,
            transform: CapturedPositionTransform::Affine { transform },
        }
    }
}

/// Canonical role of a non-modeling feature-tree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FeatureTreeNodeRole {
    /// Annotation container.
    Annotations,
    /// Ambient scene light.
    AmbientLight,
    /// Comment container.
    Comments,
    /// Cross-section container.
    CrossSections,
    /// Design-binder container.
    DesignBinder,
    /// Detail-item container.
    Details,
    /// Profile-selection handle generated from a dissectable sketch.
    DissectedProfile,
    /// Directional scene light.
    DirectionalLight,
    /// Equation container.
    Equations,
    /// Exploded-view container.
    ExplodedViews,
    /// Favorites container.
    Favorites,
    /// User-created feature folder.
    FeatureFolder,
    /// Generic history folder.
    History,
    /// Lights, cameras, and scene container.
    LightsAndCameras,
    /// Markup container.
    Markups,
    /// Built-in model origin node.
    ModelOrigin,
    /// Point scene light.
    PointLight,
    /// Material container or assignment node.
    Materials,
    /// Note container.
    Notes,
    /// Selection-set container.
    SelectionSets,
    /// Sensor container.
    Sensors,
    /// Built-in sheet-metal state root.
    SheetMetal,
    /// Solid-body container.
    SolidBodies,
    /// Spot scene light.
    SpotLight,
    /// Surface-body container.
    SurfaceBodies,
    /// Table container.
    Tables,
}
