// SPDX-License-Identifier: Apache-2.0
//! Surface continuity, boundary, trim, extension, ruled-surface, projection and wrap settings.

use super::geometry::FeatureDirection3;
use super::profiles::PathRef;
use super::selections::{EdgeSelection, NonEmptyMembers};
use super::{deserialize_filled_surface_weight, deserialize_wrap_depth};
use crate::scalar::{Length, PositiveLength, PositiveReal};
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

cadmpeg_core::named_optional_field!(
    pub(super) deserialize_continuity,
    FilledSurfaceContinuity,
    "continuity"
);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
/// Direction law for projected curves.
pub enum CurveProjectionDirection {
    /// One explicit model-space vector.
    Vector(FeatureDirection3),
    /// Direction state without one explicit vector.
    State(CurveProjectionDirectionState),
}

impl Default for CurveProjectionDirection {
    fn default() -> Self {
        Self::State(CurveProjectionDirectionState::TargetNormal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Direction state for projection without an explicit vector.
#[serde(deny_unknown_fields)]
pub enum CurveProjectionDirectionState {
    /// Projection direction remains unresolved.
    Unresolved,
    /// Project along each target face's normal.
    TargetNormal,
}

/// Material effect of a wrapped profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WrapMode {
    #[doc = "Material effect with an independently retained signed depth."]
    Unresolved {
        #[doc = "Signed depth in millimetres when available."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_wrap_depth"
        )]
        depth: Option<Length>,
    },
    /// Add material above the target face.
    Emboss {
        /// Positive normal offset above the target face.
        depth: PositiveLength,
    },
    /// Remove material below the target face.
    Deboss {
        /// Positive normal offset below the target face.
        depth: PositiveLength,
    },
    /// Imprint the profile without adding or removing material.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Scribe,
}

/// Continuity order imposed at a generated surface boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SurfaceContinuity {
    /// Positional continuity only.
    Contact,
    /// First-derivative continuity.
    Tangent,
    /// Second-derivative continuity.
    Curvature,
}

/// One filled-surface boundary condition and its continuity weight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FilledSurfaceBoundaryCondition {
    /// Positional continuity, which has no weight.
    Contact {},
    /// First-derivative continuity with an optional resolved weight.
    Tangent {
        /// Positive dimensionless weight; absent when the source weight is unresolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_filled_surface_weight"
        )]
        weight: Option<PositiveReal>,
    },
    /// Second-derivative continuity with an optional resolved weight.
    Curvature {
        /// Positive dimensionless weight; absent when the source weight is unresolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_filled_surface_weight"
        )]
        weight: Option<PositiveReal>,
    },
}

impl FilledSurfaceBoundaryCondition {
    /// The continuity order of this condition.
    #[must_use]
    pub const fn continuity(self) -> SurfaceContinuity {
        match self {
            Self::Contact {} => SurfaceContinuity::Contact,
            Self::Tangent { .. } => SurfaceContinuity::Tangent,
            Self::Curvature { .. } => SurfaceContinuity::Curvature,
        }
    }

    /// The resolved weight, absent for positional continuity.
    #[must_use]
    pub const fn weight(self) -> Option<PositiveReal> {
        match self {
            Self::Contact {} => None,
            Self::Tangent { weight } | Self::Curvature { weight } => weight,
        }
    }
}

impl From<SurfaceContinuity> for FilledSurfaceBoundaryCondition {
    fn from(continuity: SurfaceContinuity) -> Self {
        match continuity {
            SurfaceContinuity::Contact => Self::Contact {},
            SurfaceContinuity::Tangent => Self::Tangent { weight: None },
            SurfaceContinuity::Curvature => Self::Curvature { weight: None },
        }
    }
}

/// Resolved continuity conditions for a filled-surface boundary.
///
/// One condition per boundary component, in source order. A boundary whose
/// components all impose the same condition is uniform, which [`Self::uniform`]
/// reports; a uniform boundary has no second spelling to disagree with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct FilledSurfaceContinuity {
    /// Conditions of the boundary components, in source order.
    pub conditions: NonEmptyMembers<FilledSurfaceBoundaryCondition>,
}

impl FilledSurfaceContinuity {
    /// Creates one condition for the complete boundary.
    #[must_use]
    pub fn one(continuity: SurfaceContinuity) -> Self {
        Self {
            conditions: NonEmptyMembers::one(continuity.into()),
        }
    }

    /// Returns the continuity order when every component has the same order.
    #[must_use]
    pub fn uniform_continuity(&self) -> Option<SurfaceContinuity> {
        let mut conditions = self.conditions.iter();
        let first = conditions.next()?.continuity();
        conditions
            .all(|condition| condition.continuity() == first)
            .then_some(first)
    }

    /// Returns the aggregate condition when every component uses one value.
    #[must_use]
    pub fn uniform(&self) -> Option<FilledSurfaceBoundaryCondition> {
        let mut components = self.conditions.iter();
        let first = *components.next()?;
        components
            .all(|continuity| continuity == &first)
            .then_some(first)
    }
}

/// Boundary input accepted by a filled-surface operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SurfaceBoundary {
    /// Boundary selected as topological edges.
    Edges(EdgeSelection),
    /// Boundary selected as a sketch, curve, or mixed path collection.
    Path(PathRef),
}

/// Region retained by a trim-surface operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum TrimRegion {
    /// Source trim exists but the retained region is unresolved.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Unresolved,
    /// Retain the region enclosed by the trimming path.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Inside,
    /// Retain the region outside the trimming path.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Outside,
    /// Remove an explicit set of partition cells.
    Cells(TrimCellSelection),
}

/// Cells removed by a trim operation from its partition of the target faces.
///
/// Cell ordinals are one-based within the operation's source partition. The
/// total is part of the semantic value because the complement is the retained
/// result and a selected set can contain neither all nor only the first cells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TrimCellSelectionWire")]
pub struct TrimCellSelection {
    /// One-based ordinals of cells removed by the operation.
    pub(super) removed: Vec<u64>,
    /// Number of cells in the operation's partition.
    pub(super) total: u64,
}

impl TrimCellSelection {
    /// Creates a nonempty selection whose unique ordinals are within the partition.
    #[must_use]
    pub fn new(removed: Vec<u64>, total: u64) -> Option<Self> {
        let mut seen = HashSet::with_capacity(removed.len());
        (total > 0
            && !removed.is_empty()
            && removed
                .iter()
                .all(|ordinal| *ordinal > 0 && *ordinal <= total)
            && removed.iter().all(|ordinal| seen.insert(*ordinal)))
        .then_some(Self { removed, total })
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TrimCellSelectionWire {
    /// One-based ordinals of cells removed by the operation.
    removed: Vec<u64>,
    /// Number of cells in the operation's partition.
    total: u64,
}

impl TryFrom<TrimCellSelectionWire> for TrimCellSelection {
    type Error = &'static str;

    fn try_from(wire: TrimCellSelectionWire) -> Result<Self, Self::Error> {
        Self::new(wire.removed, wire.total)
            .ok_or("trim cell selection removed must be nonempty, unique, and within total")
    }
}

/// Geometric law used to extend a surface boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SurfaceExtension {
    /// Extension law is unresolved.
    Unresolved,
    /// Continue the source surface parameterization.
    Natural,
    /// Extend boundary tangents as ruled linear strips.
    Linear,
    /// Extend boundary faces perpendicular to the source faces.
    Perpendicular,
}

/// Direction law for a ruled-surface operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RuledSurfaceMode {
    /// Extend normal to the support faces.
    Normal {
        /// Positive extension distance.
        distance: PositiveLength,
    },
    /// Extend tangent to the support faces.
    Tangent {
        /// Positive extension distance.
        distance: PositiveLength,
    },
    /// Extend along one explicit model-space direction.
    Direction {
        /// Extension direction.
        direction: FeatureDirection3,
        /// Positive extension distance.
        distance: PositiveLength,
    },
}

/// Corner construction law for a ruled-surface operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RuledSurfaceCorner {
    /// Join adjacent ruled strips with rounded corners.
    Rounded,
    /// Intersect adjacent ruled strips to form mitered corners.
    Mitered,
}

/// Result topology retained by a projection-on-surface operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SurfaceProjectionMode {
    /// Retain all projected result shapes.
    All,
    /// Retain projected faces only.
    Faces,
    /// Retain projected edges only.
    Edges,
}
