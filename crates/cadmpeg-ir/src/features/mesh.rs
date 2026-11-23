use super::FeatureId;
use crate::features::geometry::{FeatureDirection3, FinitePoint3};
use crate::features::selections::{NativeSelections, SelectionMembers, SelectionReference};
use crate::scalar::{Fraction, Length, NonNegativeLength, PositiveLength, PositiveReal};
use crate::tessellation::TessellationId;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[doc = "Resolved tessellations or retained mesh source selections."]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
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

#[doc = "Construction used to convert mesh faces to a boundary representation."]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MeshConversionMethod {
    #[doc = "Creates a boundary face for each source mesh face."]
    Faceted,
    #[doc = "Combines face groups into faces of inferred prismatic features."]
    Prismatic,
}

#[doc = "Ordered inputs and construction method of a mesh-to-body conversion."]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MeshConversion {
    #[doc = "Input meshes in construction order."]
    pub sources: Vec<MeshSelection>,
    #[doc = "Conversion method, when resolved."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_conversion_method"
    )]
    pub method: Option<MeshConversionMethod>,
}

impl MeshConversion {
    #[doc = "Build a conversion from ordered sources and an optional method."]
    pub fn new(sources: Vec<MeshSelection>, method: Option<MeshConversionMethod>) -> Self {
        Self { sources, method }
    }
}

#[doc = "Distribution of the faces that a mesh operation produces."]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MeshFaceDistribution {
    #[doc = "Face size follows the local shape."]
    Adaptive,
    #[doc = "Faces have one target size."]
    Uniform,
}

#[doc = "Quantity that bounds a mesh reduction."]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshReduceTarget {
    #[doc = "Largest distance between the result and the source in canonical millimeters."]
    MaximumDeviation {
        #[doc = "Nonnegative distance bound."]
        value: NonNegativeLength,
    },
    #[doc = "Share of the source faces that the result keeps."]
    FaceProportion {
        #[doc = "Inclusive fraction from zero to one."]
        value: Fraction,
    },
    #[doc = "Number of faces in the result."]
    FaceCount {
        #[doc = "Positive face count."]
        value: std::num::NonZeroU32,
    },
}

#[doc = "Reconstruction quality of a mesh rebuild."]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MeshRebuildMode {
    #[doc = "Fast reconstruction."]
    Fast,
    #[doc = "Accurate reconstruction."]
    Accurate,
}

#[doc = "Repair applied to a mesh."]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshRepair {
    #[doc = "Fills the open boundaries."]
    CloseHoles {},
    #[doc = "Joins coincident boundaries and removes defective faces."]
    StitchAndRemove {},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Effect of an inline plane on a mesh."]
pub enum MeshPlaneCutMode {
    #[doc = "Retains the selected half-space of the mesh."]
    Trim,
    #[doc = "Partitions the mesh into bodies on both sides of the plane."]
    SplitBodies,
    #[doc = "Partitions mesh faces at the plane while retaining one body."]
    SplitFaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Faces that close mesh boundaries produced by a plane cut."]
pub enum MeshPlaneCutFill {
    #[doc = "Leaves the cut boundaries open."]
    NoFill,
    #[doc = "Closes the cut with a minimal number of faces."]
    Minimal,
    #[doc = "Closes the cut with uniformly distributed faces."]
    Uniform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Retained half-space relative to the oriented plane normal."]
pub enum MeshPlaneCutSide {
    #[doc = "Retains points whose displacement from the origin has a nonnegative dot product with the normal."]
    Positive,
    #[doc = "Retains points whose displacement from the origin has a nonpositive dot product with the normal."]
    Negative,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Independent plane and cut controls; absent fields remain unknown."]
pub struct MeshPlaneCut {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cut_origin"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "FinitePoint3"))]
    #[doc = "Point on the plane in model-space canonical millimetres."]
    pub origin: Option<FinitePoint3>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cut_normal"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "FeatureDirection3"))]
    #[doc = "Nonzero model-space normal; its magnitude is preserved."]
    pub normal: Option<FeatureDirection3>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cut_mode"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "MeshPlaneCutMode"))]
    #[doc = "Cut effect, independently of the other controls."]
    pub mode: Option<MeshPlaneCutMode>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cut_fill"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "MeshPlaneCutFill"))]
    #[doc = "Boundary fill for trim and body splitting; stored inactive values remain available."]
    pub fill: Option<MeshPlaneCutFill>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cut_side"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "MeshPlaneCutSide"))]
    #[doc = "Half-space retained by trimming; stored inactive values remain available."]
    pub retained_side: Option<MeshPlaneCutSide>,
}

cadmpeg_core::named_optional_field!(deserialize_cut_origin, FinitePoint3, "origin");
cadmpeg_core::named_optional_field!(deserialize_cut_normal, FeatureDirection3, "normal");
cadmpeg_core::named_optional_field!(deserialize_cut_mode, MeshPlaneCutMode, "mode");
cadmpeg_core::named_optional_field!(deserialize_cut_fill, MeshPlaneCutFill, "fill");
cadmpeg_core::named_optional_field!(deserialize_cut_side, MeshPlaneCutSide, "retained_side");

#[doc = "Change that a mesh operation applies to its source."]
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshModification {
    #[doc = "Separates connected shells or face groups into meshes."]
    Separate {
        #[doc = "Independent separation controls."]
        separation: MeshSeparation,
    },
    #[doc = "Cuts the source mesh with an inline oriented plane."]
    PlaneCut {
        #[doc = "Independently known plane components and cut controls."]
        cut: MeshPlaneCut,
    },
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
}
cadmpeg_core::named_optional_field!(deserialize_members, NativeSelections, "members");

#[doc = "Partition used to separate a mesh."]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MeshSeparationMethod {
    #[doc = "Separate connected shells."]
    Shells,
    #[doc = "Separate face groups."]
    FaceGroups,
}

#[doc = "Face groups selected for mesh separation, independently of the source meshes."]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshSeparationGroups {
    #[doc = "Separate all source face groups."]
    All {},
    #[doc = "Separate selected face groups whose identities can remain unresolved."]
    Selected {
        #[doc = "Ordered native group selectors relative to the mesh source."]
        #[cfg_attr(feature = "schema", schemars(with = "NativeSelections"))]
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_members"
        )]
        members: Option<NativeSelections>,
    },
}

#[doc = "Independent mesh separation settings; absent fields remain unknown."]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MeshSeparation {
    #[doc = "Separation partition when resolved."]
    #[cfg_attr(feature = "schema", schemars(with = "MeshSeparationMethod"))]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_separation_method"
    )]
    pub method: Option<MeshSeparationMethod>,
    #[doc = "Face-group scope when resolved, including inactive stored scope."]
    #[cfg_attr(feature = "schema", schemars(with = "MeshSeparationGroups"))]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_separation_groups"
    )]
    pub groups: Option<MeshSeparationGroups>,
    #[doc = "Whether face-group separation creates multiple bodies, including an inactive stored value."]
    #[cfg_attr(feature = "schema", schemars(with = "bool"))]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_multiple_bodies"
    )]
    pub multiple_bodies: Option<bool>,
    #[doc = "Whether separation keeps the source body when resolved."]
    #[cfg_attr(feature = "schema", schemars(with = "bool"))]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_keep_body"
    )]
    pub keep_body: Option<bool>,
}

cadmpeg_core::named_optional_field!(
    deserialize_separation_method,
    MeshSeparationMethod,
    "method"
);
cadmpeg_core::named_optional_field!(
    deserialize_separation_groups,
    MeshSeparationGroups,
    "groups"
);
cadmpeg_core::named_optional_field!(deserialize_multiple_bodies, bool, "multiple_bodies");
cadmpeg_core::named_optional_field!(deserialize_keep_body, bool, "keep_body");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[doc = "Algorithm used to generate mesh face groups."]
pub enum MeshFaceGroupMethod {
    #[doc = "Partition by adjacent-face angle and minimum area."]
    Fast,
    #[doc = "Fit primitive boundaries using a tolerance."]
    Accurate,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[doc = "Independent generation controls; absent fields remain unknown."]
pub struct MeshFaceGroupGenerationSettings {
    #[doc = "Generation algorithm when decoded."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_face_group_method"
    )]
    pub method: Option<MeshFaceGroupMethod>,
    #[doc = "Threshold between adjacent face normals, in radians."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle_threshold"
    )]
    pub angle_threshold: Option<crate::scalar::Angle>,
    #[doc = "Absolute distance tolerance for group boundaries, in millimetres."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boundary_tolerance"
    )]
    pub boundary_tolerance: Option<crate::scalar::NonNegativeLength>,
    #[doc = "Minimum group area as a fraction of the overall mesh area."]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_minimum_area_fraction"
    )]
    pub minimum_area_fraction: Option<crate::scalar::Fraction>,
}

cadmpeg_core::named_optional_field!(deserialize_face_group_method, MeshFaceGroupMethod, "method");
cadmpeg_core::named_optional_field!(
    deserialize_angle_threshold,
    crate::scalar::Angle,
    "angle_threshold"
);
cadmpeg_core::named_optional_field!(
    deserialize_boundary_tolerance,
    crate::scalar::NonNegativeLength,
    "boundary_tolerance"
);
cadmpeg_core::named_optional_field!(
    deserialize_minimum_area_fraction,
    crate::scalar::Fraction,
    "minimum_area_fraction"
);
cadmpeg_core::named_optional_field!(
    deserialize_conversion_method,
    MeshConversionMethod,
    "method"
);
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

mod decode_clone;

#[cfg(test)]
mod tests;
