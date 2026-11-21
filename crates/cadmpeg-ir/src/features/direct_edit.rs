// SPDX-License-Identifier: Apache-2.0
//! Draft, face motion, scale, thicken, split and flex settings.

use super::geometry::{FeatureDirection3, FeatureSupportPlaneFrame, FinitePoint3};
use super::profiles::PathRef;
use super::selections::{FaceSelection, SelectionMembers};
use super::{deserialize_form, deserialize_pull, FeatureId, EPS_DRAFT_PULL_FRAME_PARALLEL};
use crate::math::Vector3;
use crate::scalar::{Angle, Length, NonZeroReal, PositiveReal, SlopeAngle};
use crate::transform::Transform;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_frame, FeatureSupportPlaneFrame, "frame");

cadmpeg_core::named_optional_field!(deserialize_plane, FeatureId, "plane");

/// Resolved pull frame of a draft anchor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DraftPull {
    /// Pull direction used to measure the draft angle.
    pub direction: FeatureDirection3,
    /// Datum-plane feature that supplied the direction, when retained.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_plane"
    )]
    pub plane: Option<FeatureId>,
    /// Frame of the plane that supplied the direction, in the same space as
    /// `direction`, when no datum-plane feature is retained.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_frame"
    )]
    pub frame: Option<FeatureSupportPlaneFrame>,
}

impl DraftPull {
    /// Pull frame with a direction and an optional datum-plane feature.
    #[must_use]
    pub const fn new(direction: FeatureDirection3, plane: Option<FeatureId>) -> Self {
        Self {
            direction,
            plane,
            frame: None,
        }
    }

    /// Return whether the retained plane frame, if any, has a normal parallel
    /// or antiparallel to the direction.
    #[must_use]
    pub fn frame_matches_direction(&self) -> bool {
        self.frame.is_none_or(|frame| {
            let normal = frame.normal();
            let scale = normal.norm() * self.direction.norm();
            scale.is_finite()
                && normal.get().cross(self.direction.get()).norm()
                    <= EPS_DRAFT_PULL_FRAME_PARALLEL * scale
        })
    }
}

/// Draft of the face portions on the far side of a two-sided draft's neutral plane.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftSecondSide {
    /// The far portions draft by the first angle.
    Symmetric {},
    /// The far portions draft by their own signed angle.
    Angle {
        /// Signed draft angle of the far portions.
        angle: SlopeAngle,
    },
}

/// Selection form and pull frame of a draft operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftAnchor {
    /// A neutral plane remains fixed during the operation.
    NeutralPlane {
        /// Neutral-plane selection.
        plane: FaceSelection,
        /// Explicit pull frame, when the source retains one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pull"
        )]
        pull: Option<DraftPull>,
    },
    /// A parting tool and its pull frame define a parting-line draft.
    PartingLine {
        /// Parting-tool faces.
        tool: FaceSelection,
        /// Required pull frame.
        pull: DraftPull,
    },
}

impl DraftAnchor {
    /// Pull frame retained by this anchor, when available.
    #[must_use]
    pub const fn pull(&self) -> Option<&DraftPull> {
        match self {
            Self::NeutralPlane { pull, .. } => pull.as_ref(),
            Self::PartingLine { pull, .. } => Some(pull),
        }
    }
}

/// Fixed locus of a body-scale transform.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ScaleCenter {
    /// Combined centroid of the selected bodies.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Centroid,
    /// Model coordinate-system origin.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    ModelOrigin,
    /// Explicit model-space point.
    Point(FinitePoint3),
    /// Format-native coordinate-system or reference identifier.
    Native(String),
}

/// Factors of a body-scale transform.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ScaleFactors {
    /// No complete scale factor is available.
    Unresolved {},
    /// One factor applies on all three axes.
    Uniform {
        /// The one factor.
        factor: NonZeroReal,
    },
    /// Independent factors apply on the model-space axes.
    PerAxis {
        /// Factors on the model-space axes, in order.
        factors: [NonZeroReal; 3],
    },
}

impl ScaleFactors {
    /// Resolve the effective model-space factors when construction is complete.
    #[must_use]
    pub fn resolved(self) -> Option<Vector3> {
        match self {
            Self::Unresolved {} => None,
            Self::Uniform { factor } => {
                Some(Vector3::new(factor.get(), factor.get(), factor.get()))
            }
            Self::PerAxis { factors } => Some(Vector3::new(
                factors[0].get(),
                factors[1].get(),
                factors[2].get(),
            )),
        }
    }
}

/// Direction in which a thicken feature adds material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ThickenSide {
    /// Add material along the selected-face normal.
    Forward,
    /// Add material opposite the selected-face normal.
    Reverse,
    /// Split the thickness equally across both sides.
    Both,
}

/// Direct face-motion law.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FaceMotion {
    /// Offset along each face normal.
    Offset {
        /// Signed offset distance.
        distance: Length,
    },
    /// Translation along one direction.
    Translate {
        /// Translation direction.
        direction: FeatureDirection3,
        /// Signed translation distance.
        distance: Length,
    },
    /// Rotation about an axis.
    Rotate {
        /// Point on the rotation axis.
        axis_origin: FinitePoint3,
        /// Rotation-axis direction.
        axis_dir: FeatureDirection3,
        /// Signed rotation angle.
        angle: Angle,
    },
    #[doc = "Complete model-space affine motion in canonical millimetres."]
    Transform {
        #[doc = "Row-major transform applied to the selected faces."]
        transform: Transform,
    },
}

/// Model-space axis-angle rotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AxisAngle {
    /// Point on the rotation axis.
    pub origin: FinitePoint3,
    /// Rotation-axis direction.
    pub direction: FeatureDirection3,
    /// Signed rotation angle.
    pub angle: Angle,
}

/// At least two distinct datum-plane feature identities in source order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SplitFacePlanes(pub(super) SelectionMembers<FeatureId>);

impl TryFrom<Vec<FeatureId>> for SplitFacePlanes {
    type Error = &'static str;
    fn try_from(planes: Vec<FeatureId>) -> Result<Self, Self::Error> {
        if planes.len() < 2 {
            return Err("planes must contain at least two planes");
        }
        Ok(Self(
            planes.try_into().map_err(|_| "planes must be distinct")?,
        ))
    }
}

impl std::ops::Deref for SplitFacePlanes {
    type Target = [FeatureId];
    fn deref(&self) -> &[FeatureId] {
        &self.0
    }
}

impl<'a> IntoIterator for &'a SplitFacePlanes {
    type Item = &'a FeatureId;
    type IntoIter = std::slice::Iter<'a, FeatureId>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de> Deserialize<'de> for SplitFacePlanes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Vec::<FeatureId>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Geometry used to partition bodies in a `SplitBody` operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SplitBodyTool {
    /// Surface faces used to partition the targets.
    Faces(FaceSelection),
    /// Datum-plane feature extended through the target bodies.
    Plane {
        /// Earlier datum-plane feature supplying the splitting plane.
        plane: FeatureId,
    },
}

/// Geometry used to partition faces in a `SplitFace` operation.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SplitFaceTool {
    #[doc = "Selected surface faces used as the splitting tool."]
    Faces(FaceSelection),
    #[doc = "Native splitting geometry whose neutral kind is unresolved."]
    Native(String),
    /// Sketch or model-space path projected onto the target faces.
    Path(PathRef),
    /// Datum-plane feature extended through the target faces.
    Plane {
        /// Earlier datum-plane feature supplying the splitting plane.
        plane: FeatureId,
    },
    /// Two or more datum-plane features extended through the target faces.
    Planes {
        /// Unique earlier datum-plane features in operation order.
        planes: SplitFacePlanes,
    },
}

/// Deformation applied by a flex feature.
///
/// Each resolved arm carries the one magnitude its family needs, and the
/// unresolved arm carries no magnitude at all, so a magnitude beside an
/// unresolved family has no spelling.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FlexMode {
    /// Deformation family whose required magnitude remains unresolved.
    Unresolved {
        /// Deformation family, when the source named one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_form"
        )]
        form: Option<FlexForm>,
    },
    /// Bend through a signed angle.
    Bending {
        /// Total bend angle.
        angle: Angle,
    },
    /// Twist through a signed angle.
    Twisting {
        /// Total twist angle.
        angle: Angle,
    },
    /// Scale transverse sections by a dimensionless factor.
    Tapering {
        /// End-to-start transverse scale ratio.
        factor: PositiveReal,
    },
    /// Extend or contract along the flex axis.
    Stretching {
        /// Signed change in length.
        distance: Length,
    },
}

/// Structural form of a flex deformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FlexForm {
    /// Angular bending.
    Bending,
    /// Angular twisting.
    Twisting,
    /// Transverse tapering.
    Tapering,
    /// Axial stretching.
    Stretching,
}
