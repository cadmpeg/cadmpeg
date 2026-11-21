// SPDX-License-Identifier: Apache-2.0

use super::{EdgeSelection, FaceSelection, FeatureOperation, FiniteVector3, ThickenSide};
use crate::scalar::PositiveLength;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::Serialize;

#[doc = "Trim loops removed by a surface untrim."]
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum UntrimLoops {
    #[doc = "Loop choice is unresolved."]
    Unresolved {},
    #[doc = "Every trim loop of each selected face."]
    All {},
    #[doc = "The interior trim loops of each selected face."]
    Internal {},
    #[doc = "The exterior trim loop of each selected face."]
    External {},
    #[doc = "The selected trim loops."]
    Selected {
        #[doc = "Edges of the selected trim loops."]
        edges: EdgeSelection,
    },
}

/// Result of partitioning a body along its silhouette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SilhouetteSplitMode {
    /// The result mode is unresolved.
    Unresolved,
    /// Partition the faces while preserving the body.
    FacesOnly,
    /// Partition a hollow body with new internal faces.
    ShelledBody,
    /// Partition a solid body into solid bodies.
    SolidBody,
}

impl FeatureOperation {
    #[doc = "Construct a surface untrim with unresolved loop and extension settings."]
    pub fn untrim_surface(faces: FaceSelection) -> Self {
        Self::UntrimSurface {
            faces,
            loops: UntrimLoops::Unresolved {},
            extension: None,
        }
    }

    #[doc = "Construct a Thicken with unspecified result and tangent-chain settings."]
    pub fn thicken(
        faces: FaceSelection,
        thickness: Option<PositiveLength>,
        side: Option<ThickenSide>,
    ) -> Self {
        Self::Thicken {
            faces,
            thickness,
            side,
            result: None,
            tangent_chain: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[doc = "Projection method used by a `SplitFace` operation."]
pub enum SplitFaceMethod {
    #[doc = "Splitting method remains unresolved."]
    Unresolved {},
    #[doc = "Intersect target faces with the splitting tool's surfaces."]
    SurfaceIntersection {
        #[doc = "Extend the splitting tool beyond its existing boundary."]
        extend: bool,
    },
    #[doc = "Project the splitting tool onto the closest target points."]
    ClosestPoint {},
    #[doc = "Project the splitting tool along a selected direction."]
    AlongVector {
        #[doc = "Decoded finite projection vector, including degenerate triples."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_split_face_direction"
        )]
        direction: Option<FiniteVector3>,
        #[doc = "Original source label of the unresolved direction carrier."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_split_face_native_carrier"
        )]
        native_carrier: Option<String>,
        #[doc = "Original source labels and null slots, in encoded input order."]
        native_inputs: Vec<Option<String>>,
        #[doc = "Declared number of direction-source input slots."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_split_face_native_input_count"
        )]
        native_input_count: Option<u32>,
        #[doc = "Whether the declared input run was read completely."]
        native_inputs_complete: bool,
    },
}

impl Default for SplitFaceMethod {
    fn default() -> Self {
        Self::Unresolved {}
    }
}

cadmpeg_core::named_optional_field!(deserialize_split_face_direction, FiniteVector3, "direction");
cadmpeg_core::named_optional_field!(
    deserialize_split_face_native_carrier,
    String,
    "native_carrier"
);
cadmpeg_core::named_optional_field!(
    deserialize_split_face_native_input_count,
    u32,
    "native_input_count"
);

#[cfg(test)]
mod tests;
