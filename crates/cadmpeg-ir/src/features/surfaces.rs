// SPDX-License-Identifier: Apache-2.0

use super::{EdgeSelection, FaceSelection, FeatureOperation, ThickenSide};
use crate::scalar::PositiveLength;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[doc = "Trim loops removed by a surface untrim."]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests;
