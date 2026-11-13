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

mod decode_clone;

#[cfg(test)]
mod tests;
