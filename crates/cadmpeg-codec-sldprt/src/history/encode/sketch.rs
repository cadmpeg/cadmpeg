// SPDX-License-Identifier: Apache-2.0
//! Sketch, spatial-sketch, sketch-block, and wrap write encoders.

use super::super::literals::format_length_mm;
use super::support::{face_selection_value, planar_profile_source, require_same_family};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::NativeClassKind;
use crate::history::classify::feature_input_class;
use crate::writer::write_error::WriteError;
use cadmpeg_ir::features::{FaceSelection, FeatureId, PlanarProfileRef, WrapMode};

impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_sketch_block_definition(&self) -> NeutralFeatureEncoding {
        let record = self
            .existing
            .filter(|record| feature_input_class(record, NativeClassKind::SketchBlockDefinition));
        NeutralFeatureEncoding {
            kind: record.map_or_else(
                || "SketchBlockDefinition".into(),
                |record| record.kind.clone(),
            ),
            parameters: self.base_parameters(),
            properties: self.base_properties(),
        }
    }

    pub(super) fn encode_sketch_block_instance(
        &self,
        block: Option<&FeatureId>,
        placement: Option<&cadmpeg_ir::transform::Transform>,
    ) -> Result<NeutralFeatureEncoding, WriteError> {
        if block.is_some() || placement.is_some() {
            return Err(WriteError::Decline(format!(
                "SLDPRT feature {} requires a native sketch-block instance record",
                self.feature.id
            )));
        }
        let record = self
            .existing
            .filter(|record| feature_input_class(record, NativeClassKind::SketchBlockInstance));
        let mut properties = self.base_properties();
        properties.remove("BlockDefinition");
        properties.remove("BlockOrigin");
        Ok(NeutralFeatureEncoding {
            kind: record.map_or_else(
                || "SketchBlockInstance".into(),
                |record| record.kind.clone(),
            ),
            parameters: self.base_parameters(),
            properties,
        })
    }

    pub(super) fn encode_wrap(
        &self,
        profile: &PlanarProfileRef,
        face: &FaceSelection,
        mode: &WrapMode,
    ) -> Result<NeutralFeatureEncoding, WriteError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            require_same_family(existing, &feature.id, &["Wrap"])?;
            let profile =
                planar_profile_source(profile, record_sources, feature_sources, sketch_sources)
                    .ok_or_else(|| {
                        WriteError::Decline(format!(
                            "SLDPRT feature {} references a missing wrap profile",
                            feature.id
                        ))
                    })?;
            let face = face_selection_value(face).ok_or_else(|| {
                WriteError::Decline(format!(
                    "SLDPRT feature {} has no wrap target face",
                    feature.id
                ))
            })?;
            let mut parameters = self.base_parameters();
            match mode {
                WrapMode::Emboss { depth } | WrapMode::Deboss { depth } => {
                    parameters.insert(
                        cadmpeg_core::nonblank_literal!("Depth"),
                        format_length_mm(self.dimension(depth)?.into()),
                    );
                }
                WrapMode::Scribe => {
                    parameters.remove("Depth");
                }
            }
            let mut properties = self.base_properties();
            properties.insert(cadmpeg_core::nonblank_literal!("Profile"), profile);
            properties.insert(cadmpeg_core::nonblank_literal!("Face"), face);
            properties.insert(
                cadmpeg_core::nonblank_literal!("Mode"),
                match mode {
                    WrapMode::Emboss { .. } => "Emboss",
                    WrapMode::Deboss { .. } => "Deboss",
                    WrapMode::Scribe => "Scribe",
                }
                .into(),
            );
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Wrap".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_sketch(&self) -> Result<NeutralFeatureEncoding, WriteError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            require_same_family(existing, &feature.id, &["Sketch"])?;
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Sketch".into(), |record| record.kind.clone()),
                parameters: self.base_parameters(),
                properties: self.base_properties(),
            }
        })
    }

    pub(super) fn encode_spatial_sketch(&self) -> Result<NeutralFeatureEncoding, WriteError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            require_same_family(existing, &feature.id, &["Sketch"])?;
            NeutralFeatureEncoding {
                kind: "3DSketch".into(),
                parameters: self.base_parameters(),
                properties: self.base_properties(),
            }
        })
    }
}
