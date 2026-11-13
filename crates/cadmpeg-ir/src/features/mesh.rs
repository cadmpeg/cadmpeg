use super::{FeatureId, NativeSelections, SelectionMembers, SelectionReference};
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

cadmpeg_core::named_optional_field!(deserialize_members, NativeSelections, "members");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
        deserialize_with = "deserialize_method"
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

cadmpeg_core::named_optional_field!(deserialize_method, MeshFaceGroupMethod, "method");
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

mod decode_clone;

#[cfg(test)]
mod tests;
