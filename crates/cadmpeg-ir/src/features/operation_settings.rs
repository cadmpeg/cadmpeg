// SPDX-License-Identifier: Apache-2.0
//! Boolean, shell, fuzzy-tolerance and body-retention settings.

use super::selections::BodySelection;
use super::sweeps::SolidSweepOperation;
use crate::scalar::PositiveLength;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::Serialize;

#[doc = "Component creation mode of a Boolean combination."]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CombineDestination {
    #[doc = "Keep the result in the target body's component."]
    #[default]
    TargetComponent,
    #[doc = "Create a component that contains the combined result."]
    NewComponent,
}

/// External model format consumed by an imported-geometry feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum GeometryImportFormat {
    /// ISO 10303 STEP model data.
    Step,
    /// IGES model data.
    Iges,
    /// Native boundary-representation model data.
    Brep,
}

/// Selection policy for Boolean-operation fuzzy tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FuzzyTolerance {
    /// Let the modeling kernel use its default tolerance.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    KernelDefault,
    /// Determine a suitable tolerance from the participating shapes.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Automatic,
    /// Use the supplied positive model-unit tolerance.
    Explicit(PositiveLength),
}

/// Geometric offset construction used by a thin-wall shell operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ShellMode {
    /// Offsets the selected boundary as a skin.
    Skin,
    /// Extends the offset along boundary edges as a pipe-like wall.
    Pipe,
    /// Builds wall material on both sides of the original boundary.
    BothSides,
}

/// Corner continuation law for adjacent shell offset faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ShellJoin {
    /// Continues corners with rounded arcs.
    Arc,
    /// Extends adjacent faces tangentially to meet.
    Tangent,
    /// Intersects adjacent offset faces to form sharp corners.
    Intersection,
}

/// Traversal relationship between ruled-surface boundary curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RuledCurveOrientation {
    /// Select curve traversal automatically from endpoint proximity.
    Automatic,
    /// Retain both persisted curve traversal directions.
    Forward,
    /// Reverse the second curve relative to the first.
    Reversed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Side retained by a body-trim operation.
#[serde(deny_unknown_fields)]
pub enum BodyTrimSide {
    /// Retained side is unresolved.
    Unresolved,
    /// Retain the side selected by tool orientation.
    Forward,
    /// Retain the side opposite tool orientation.
    Reverse,
}

/// Selection interpretation for a delete/keep-body operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BodyRetentionMode {
    /// The operation family is known but the selected retention mode is unavailable.
    Unresolved,
    /// Delete the selected bodies.
    DeleteSelected,
    /// Delete every body except the selected bodies.
    KeepSelected,
}

/// Boolean effect of a solid-producing feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BooleanOp {
    /// Source operation is retained but not semantically resolved.
    Unresolved,
    /// Union with existing bodies.
    Join,
    /// Subtraction from existing bodies.
    Cut,
    /// Intersection with existing bodies.
    Intersect,
    /// Creates an independent new body without combining.
    NewBody,
}

/// Boolean operation that consumes at least one existing target body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BooleanKind {
    /// Union with existing bodies.
    Join,
    /// Subtraction from existing bodies.
    Cut,
    /// Intersection with existing bodies.
    Intersect,
}

impl From<BooleanKind> for BooleanOp {
    fn from(value: BooleanKind) -> Self {
        match value {
            BooleanKind::Join => Self::Join,
            BooleanKind::Cut => Self::Cut,
            BooleanKind::Intersect => Self::Intersect,
        }
    }
}

impl TryFrom<BooleanOp> for BooleanKind {
    type Error = BooleanOp;

    fn try_from(value: BooleanOp) -> Result<Self, Self::Error> {
        match value {
            BooleanOp::Join => Ok(Self::Join),
            BooleanOp::Cut => Ok(Self::Cut),
            BooleanOp::Intersect => Ok(Self::Intersect),
            BooleanOp::Unresolved | BooleanOp::NewBody => Err(value),
        }
    }
}

impl From<SolidSweepOperation> for BooleanOp {
    fn from(value: SolidSweepOperation) -> Self {
        match value {
            SolidSweepOperation::NewBody => Self::NewBody,
            SolidSweepOperation::Join => Self::Join,
            SolidSweepOperation::Cut => Self::Cut,
            SolidSweepOperation::Intersect => Self::Intersect,
        }
    }
}

#[doc = "Result semantics of a solid-producing feature."]
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SolidFeatureResult {
    /// Create an independent body.
    NewBody {},
    #[doc = "Combine the produced volume with selected existing bodies."]
    Boolean {
        /// Join, cut, or intersection operation.
        operation: BooleanKind,
        /// Existing bodies participating in the operation.
        targets: BodySelection,
    },
}
