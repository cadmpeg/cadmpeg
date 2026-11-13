use super::{FeatureId, NativeSelections, SelectionMembers, SelectionReference};
use crate::scalar::{Fraction, Length, NonNegativeLength, PositiveLength, PositiveReal};
use crate::tessellation::TessellationId;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[doc = "Resolved tessellations or retained mesh source selections."]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(untagged, deny_unknown_fields)]
pub enum MeshSelection {
    #[doc = "Nonempty distinct local tessellation identities in source order."]
    Tessellations(SelectionMembers<TessellationId>),
    #[doc = "Native source with optional member selectors whose geometry remains unresolved."]
    Native {
        #[doc = "Native expression identifying the source selection."]
        native: SelectionReference,
        #[doc = "Ordered member selectors relative to the native source."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_members"
        )]
        members: Option<NativeSelections>,
    },
    #[doc = "Unavailable source identified by the feature that retains its construction."]
    Unresolved {
        #[doc = "Originating feature whose mesh source remains unavailable."]
        feature: FeatureId,
    },
}

#[doc = "Distribution of the faces that a mesh operation produces."]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MeshFaceDistribution {
    #[doc = "Face size follows the local shape."]
    Adaptive,
    #[doc = "Faces have one target size."]
    Uniform,
}

#[doc = "Quantity that bounds a mesh reduction."]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MeshReduceTarget {
    #[doc = "Largest distance between the result and the source in canonical millimeters."]
    MaximumDeviation(NonNegativeLength),
    #[doc = "Share of the source faces that the result keeps."]
    FaceProportion(Fraction),
    #[doc = "Number of faces in the result."]
    FaceCount(std::num::NonZeroU32),
}

#[doc = "Reconstruction quality of a mesh rebuild."]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MeshRebuildMode {
    #[doc = "Fast reconstruction."]
    Fast,
    #[doc = "Accurate reconstruction."]
    Accurate,
}

#[doc = "Repair applied to a mesh."]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshRepair {
    #[doc = "Fills the open boundaries."]
    CloseHoles,
    #[doc = "Joins coincident boundaries and removes defective faces."]
    StitchAndRemove,
    #[doc = "Reconstructs the mesh from its volume."]
    Rebuild {
        #[doc = "Reconstruction quality, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_mode"
        )]
        mode: Option<MeshRebuildMode>,
        #[doc = "Sampling density of the reconstruction, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_density"
        )]
        density: Option<PositiveReal>,
        #[doc = "Signed surface offset in canonical millimeters, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Length>,
    },
}

#[doc = "Change that a mesh operation applies to its source."]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshModification {
    #[doc = "Lowers the face count."]
    Reduce {
        #[doc = "Face distribution of the result, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distribution"
        )]
        distribution: Option<MeshFaceDistribution>,
        #[doc = "Active bound of the reduction, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_target"
        )]
        target: Option<MeshReduceTarget>,
    },
    #[doc = "Repairs defects."]
    Repair {
        #[doc = "Repair method, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_method"
        )]
        method: Option<MeshRepair>,
    },
    #[doc = "Replaces the faces with faces of a target edge length."]
    Remesh {
        #[doc = "Face distribution of the result, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distribution"
        )]
        distribution: Option<MeshFaceDistribution>,
        #[doc = "Target edge length in canonical millimeters, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_edge_length"
        )]
        edge_length: Option<PositiveLength>,
        #[doc = "Weight that keeps sharp source shape under an adaptive distribution."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_shape_preservation"
        )]
        shape_preservation: Option<Fraction>,
    },
    #[doc = "Smooths the surface."]
    Smooth {
        #[doc = "Smoothing strength, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_smoothness"
        )]
        smoothness: Option<Fraction>,
    },
    #[doc = "Hollows the mesh and keeps a wall."]
    Shell {
        #[doc = "Wall thickness in canonical millimeters, when resolved."]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thickness"
        )]
        thickness: Option<PositiveLength>,
    },
    #[doc = "Reverses the face normals."]
    ReverseNormals,
}
cadmpeg_core::named_optional_field!(deserialize_members, NativeSelections, "members");

cadmpeg_core::named_optional_field!(deserialize_mode, MeshRebuildMode, "mode");
cadmpeg_core::named_optional_field!(deserialize_density, PositiveReal, "density");
cadmpeg_core::named_optional_field!(deserialize_offset, Length, "offset");
cadmpeg_core::named_optional_field!(
    deserialize_distribution,
    MeshFaceDistribution,
    "distribution"
);
cadmpeg_core::named_optional_field!(deserialize_target, MeshReduceTarget, "target");
cadmpeg_core::named_optional_field!(deserialize_method, MeshRepair, "method");
cadmpeg_core::named_optional_field!(deserialize_edge_length, PositiveLength, "edge_length");
cadmpeg_core::named_optional_field!(
    deserialize_shape_preservation,
    Fraction,
    "shape_preservation"
);
cadmpeg_core::named_optional_field!(deserialize_smoothness, Fraction, "smoothness");
cadmpeg_core::named_optional_field!(deserialize_thickness, PositiveLength, "thickness");

mod decode_clone;

#[cfg(test)]
mod tests;
