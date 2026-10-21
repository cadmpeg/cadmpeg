// SPDX-License-Identifier: Apache-2.0
//! Native parameter names that also determine an operation's construction.

use super::NeutralFeatureEncoder;
use crate::history::classify::indexed_name;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{edge_treatments::RadiusSpec, FeatureOperation, RibDraft, ScaleFactors};

impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn require_representable_independent_parameters(&self) -> Result<(), CodecError> {
        let operation = self.feature.definition().operation();
        for parameter in self
            .canonical_parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&self.feature.id))
        {
            let name = parameter.name.as_str();
            let conflict = match operation {
                FeatureOperation::Rib { construction, .. }
                    if construction.draft == RibDraft::None && name == "Draft" =>
                {
                    Some("rib draft")
                }
                FeatureOperation::KnitSurface {
                    gap_tolerance: None,
                    ..
                } if name == "GapTolerance" => Some("knit gap tolerance"),
                FeatureOperation::Scale {
                    factors: ScaleFactors::PerAxis { .. },
                    ..
                } if name == "Factor" => Some("uniform scale"),
                FeatureOperation::Scale {
                    factors: ScaleFactors::Uniform { .. },
                    ..
                } if matches!(name, "ScaleX" | "ScaleY" | "ScaleZ") => Some("per-axis scale"),
                FeatureOperation::Fillet { groups }
                    if groups
                        .iter()
                        .any(|group| matches!(group.radius, RadiusSpec::Constant { .. }))
                        && (indexed_name(name, "Position") || indexed_name(name, "Radius")) =>
                {
                    Some("variable fillet radius law")
                }
                _ => None,
            };
            if let Some(construction) = conflict {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} cannot retain independent parameter {} without reactivating its {construction}",
                    self.feature.id, parameter.name
                )));
            }
        }
        Ok(())
    }
}
