// SPDX-License-Identifier: Apache-2.0
//! Extrude starts, linear and angular terminations, and extrusion directions.

use super::geometry::{FeatureDirection3, FeatureSupportPlaneFrame};
use super::profiles::PathRef;
use super::selections::{FaceSelection, VertexSelection};
use super::{
    deserialize_draft, deserialize_offset, deserialize_plane_target_source, deserialize_source,
};
use crate::scalar::{Length, NonZeroLength, PositiveAngle, PositiveLength, SlopeAngle};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Direction in which an extrusion sweeps its profile.
#[derive(Clone, Debug, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtrudeDirection {
    /// Native direction selection is present structurally but unresolved.
    Unresolved {},
    /// Sweep along the profile's positive normal.
    ProfileNormal {},
    /// Sweep opposite the profile's positive normal.
    ReversedProfileNormal {},
    /// Sweep along an explicit model-space vector.
    Explicit {
        /// Directed model-space sweep vector.
        vector: FeatureDirection3,
        /// Persisted selection or rule used to resolve the vector, when retained.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_source"
        )]
        source: Option<ExtrusionDirectionSource>,
    },
}

impl Default for ExtrudeDirection {
    fn default() -> Self {
        Self::ProfileNormal {}
    }
}

/// Start condition of a linear extrusion.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtrudeStart {
    /// Native start condition is present structurally but unresolved.
    Unresolved {},
    /// Begin on the profile's own plane.
    ProfilePlane {},
    /// Begin on a plane parallel to the profile plane at a signed offset.
    OffsetProfilePlane {
        /// Signed offset along the profile normal in canonical millimeters.
        offset: Length,
    },
    /// Begin on a selected face, optionally displaced along the extrusion direction.
    FromFace {
        /// Face defining the start plane.
        face: FaceSelection,
        /// Signed displacement from the selected face.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Length>,
    },
    /// Begin on a plane with its native target reference.
    FromPlane {
        /// Model-space frame of the start plane.
        plane: FeatureSupportPlaneFrame,
        /// Native target record.
        native: NonBlankString,
        /// Source of the target plane.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_plane_target_source"
        )]
        source: Option<PlaneTargetSource>,
    },
}

/// Source of a resolved feature support plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlaneTargetSource {
    /// A work plane supplies the frame.
    WorkPlane,
    /// A work point supplies the plane origin.
    WorkPoint,
    /// A stored planar face surface supplies the frame; its face identity remains native.
    FaceSurface,
}

/// One-sided termination law of a linear sweep. Sidedness around the profile
/// plane is stated by the owning feature's extent type.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinearTermination {
    /// Native termination is present structurally but unresolved.
    Unresolved {},
    /// Fixed travel distance.
    Blind {
        /// Fixed travel distance.
        length: NonZeroLength,
    },
    /// Extends through all material.
    ThroughAll {},
    /// Extends until it exits the next material region.
    ThroughNext {},
    /// Extends until the first encountered model face.
    ToFirst {},
    /// Extends until the last encountered model face.
    ToLast {},
    /// Extends until it reaches a target face.
    ToFace {
        /// Face terminating the operation.
        face: FaceSelection,
        /// Signed displacement from the terminating face.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Length>,
    },
    /// Extends until it reaches a target vertex.
    ToVertex {
        /// Vertex terminating the operation.
        vertex: VertexSelection,
    },
    /// Extends until it reaches a plane with its native target reference.
    ToPlane {
        /// Model-space frame of the terminating plane.
        plane: FeatureSupportPlaneFrame,
        /// Native target record.
        native: NonBlankString,
        /// Source of the target plane.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_plane_target_source"
        )]
        source: Option<PlaneTargetSource>,
    },
    /// Extends to a fixed offset from a target face.
    OffsetFromFace {
        /// Face the termination is measured from.
        face: FaceSelection,
        /// Offset distance from the face.
        offset: PositiveLength,
    },
    /// Extends until one of the faces in a selected target shape.
    ToShape {
        /// Native or resolved target shape selection.
        target: FaceSelection,
    },
}

/// One-sided termination law of an angular sweep. Sidedness around the profile
/// plane is stated by the owning revolution extent.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AngularTermination {
    /// Native termination is present structurally but unresolved.
    Unresolved {},
    /// Extends through all material.
    ThroughAll {},
    /// Extends until it exits the next material region.
    ThroughNext {},
    /// Extends until the first encountered model face.
    ToFirst {},
    /// Extends until the last encountered model face.
    ToLast {},
    /// Extends until it reaches a target face.
    ToFace {
        /// Face terminating the operation.
        face: FaceSelection,
        /// Signed displacement from the terminating face.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Length>,
    },
    /// Extends until it reaches a target vertex.
    ToVertex {
        /// Vertex terminating the operation.
        vertex: VertexSelection,
    },
    /// Extends to a fixed offset from a target face.
    OffsetFromFace {
        /// Face the termination is measured from.
        face: FaceSelection,
        /// Offset distance from the face.
        offset: PositiveLength,
    },
    /// Extends until one of the faces in a selected target shape.
    ToShape {
        /// Native or resolved target shape selection.
        target: FaceSelection,
    },
    /// Extends until it reaches a plane with its native target reference.
    ToPlane {
        /// Model-space frame of the terminating plane.
        plane: FeatureSupportPlaneFrame,
        /// Native target record.
        native: NonBlankString,
        /// Source of the target plane.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_plane_target_source"
        )]
        source: Option<PlaneTargetSource>,
    },
    /// Fixed angular travel.
    Angle {
        /// Angular travel.
        angle: PositiveAngle,
    },
}

/// One side of an extrusion: its termination law and side-local modifiers.
/// Drafts are measured from the profile plane outward along the side's
/// travel; an absent draft leaves the side walls parallel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExtrudeSide {
    /// Where this side's travel terminates.
    pub termination: LinearTermination,
    /// Draft angle applied to this side's walls, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_draft"
    )]
    pub draft: Option<SlopeAngle>,
}

impl Default for ExtrudeStart {
    fn default() -> Self {
        Self::ProfilePlane {}
    }
}

/// Extrusion sidedness around the profile plane.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtrudeExtent {
    /// Travel on the oriented side only.
    OneSided {
        /// The single traveled side.
        side: ExtrudeSide,
    },
    /// Independent sides on each side of the profile plane.
    TwoSided {
        /// Side along the extrusion direction.
        first: ExtrudeSide,
        /// Side opposite the extrusion direction.
        second: ExtrudeSide,
    },
    /// One side mirrored across the profile plane. A blind length states the
    /// total travel split evenly around the plane.
    Symmetric {
        /// The mirrored side.
        side: ExtrudeSide,
    },
}

/// Persisted source of a resolved linear-extrusion direction.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtrusionDirectionSource {
    /// Direction comes from the persisted direction vector.
    Custom {},
    /// Direction comes from a selected straight edge.
    Edge {
        /// Native edge selection used as the direction axis.
        reference: PathRef,
    },
    /// Direction comes from the source profile's plane normal.
    ProfileNormal {},
}
