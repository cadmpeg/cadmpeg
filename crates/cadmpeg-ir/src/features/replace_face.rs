// SPDX-License-Identifier: Apache-2.0

use super::selection_overlap::face_selections_overlap;
use super::FaceSelection;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[doc = "Face selections with no known overlap and tangent-chain control for face replacement."]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ReplaceFaceOperands {
    #[doc = "Faces whose surface geometry is replaced."]
    targets: FaceSelection,
    #[doc = "Faces supplying replacement surface geometry."]
    replacements: FaceSelection,
    #[doc = "Whether tangent faces join the target selection."]
    #[cfg_attr(feature = "schema", schemars(with = "bool"))]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangent_chain"
    )]
    tangent_chain: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceFaceOperandsWire {
    #[doc = "Faces whose surface geometry is replaced."]
    targets: FaceSelection,
    #[doc = "Faces supplying replacement surface geometry."]
    replacements: FaceSelection,
    #[doc = "Whether tangent faces join the target selection."]
    #[serde(default, deserialize_with = "deserialize_tangent_chain")]
    tangent_chain: Option<bool>,
}

cadmpeg_core::named_optional_field!(deserialize_tangent_chain, bool, "tangent_chain");

impl ReplaceFaceOperands {
    #[doc = "Admit selections with no known overlap and unspecified tangent-chain control."]
    pub fn new(targets: FaceSelection, replacements: FaceSelection) -> Result<Self, &'static str> {
        if face_selections_overlap(&targets, &replacements) {
            return Err("targets and replacements must have valid arity and disjoint membership");
        }
        Ok(Self {
            targets,
            replacements,
            tangent_chain: None,
        })
    }

    #[doc = "Return the faces whose surface geometry is replaced."]
    pub fn targets(&self) -> &FaceSelection {
        &self.targets
    }

    #[doc = "Return the replacement support faces."]
    pub fn replacements(&self) -> &FaceSelection {
        &self.replacements
    }

    #[doc = "Return the target tangent-chain control."]
    pub fn tangent_chain(&self) -> Option<bool> {
        self.tangent_chain
    }

    #[doc = "Set the target tangent-chain control."]
    #[must_use]
    pub fn with_tangent_chain(mut self, tangent_chain: Option<bool>) -> Self {
        self.tangent_chain = tangent_chain;
        self
    }

    #[doc = "Admit both edited selections while retaining their tangent-chain control."]
    pub fn try_edit(
        &mut self,
        edit: impl FnOnce(&mut FaceSelection, &mut FaceSelection),
    ) -> Result<(), &'static str> {
        let mut targets = self.targets.clone();
        let mut replacements = self.replacements.clone();
        edit(&mut targets, &mut replacements);
        *self = Self::new(targets, replacements)?.with_tangent_chain(self.tangent_chain);
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ReplaceFaceOperands {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ReplaceFaceOperandsWire::deserialize(deserializer)?;
        Self::new(wire.targets, wire.replacements)
            .map(|operands| operands.with_tangent_chain(wire.tangent_chain))
            .map_err(serde::de::Error::custom)
    }
}

rewrite_record!(ReplaceFaceOperands, []; { targets, replacements, tangent_chain });

#[cfg(test)]
mod tests;
