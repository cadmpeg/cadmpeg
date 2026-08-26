// SPDX-License-Identifier: Apache-2.0
//! Sketch, spatial-sketch, sketch-block, and wrap write encoders.

use super::super::{format_length_mm, sketch_block_placement};
use super::support::{face_selection_value, profile_source, require_same_family};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::NativeClassKind;
use crate::history::classify::feature_input_class;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FaceSelection, FeatureId, Length, ProfileRef, WrapMode};

#[allow(
    clippy::too_many_arguments,
    clippy::trivially_copy_pass_by_ref,
    clippy::ref_option,
    clippy::ptr_arg,
    reason = "Encoder arguments are borrowed from one FeatureDefinition match."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_sketch_block_definition(
        &self,
        sketch: &Option<cadmpeg_ir::sketches::SketchId>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            if sketch.is_some() {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes sketch-block geometry",
                    feature.id
                )));
            }
            let record = existing.filter(|record| {
                feature_input_class(record, NativeClassKind::SketchBlockDefinition)
            });
            let Some(record) = record else {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires a retained sketch-block definition",
                    feature.id
                )));
            };
            NeutralFeatureEncoding {
                kind: record.kind.clone(),
                parameters: record.parameters.clone(),
                properties: record.properties.clone(),
            }
        })
    }

    pub(super) fn encode_sketch_block_instance(
        &self,
        block: &Option<FeatureId>,
        placement: &Option<cadmpeg_ir::transform::Transform>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let feature_sources = self.feature_sources;
        Ok({
            let retained_source = existing
                .and_then(|record| record.properties.get("BlockDefinition"))
                .map(String::as_str);
            let block_source = block
                .as_ref()
                .and_then(|block| feature_sources.get(block).copied());
            let retained_placement = existing.and_then(sketch_block_placement);
            if retained_source != block_source || retained_placement.as_ref() != placement.as_ref()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes sketch-block instance semantics",
                    feature.id
                )));
            }
            let record = existing
                .filter(|record| feature_input_class(record, NativeClassKind::SketchBlockInstance));
            let Some(record) = record else {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires a retained sketch-block instance",
                    feature.id
                )));
            };
            NeutralFeatureEncoding {
                kind: record.kind.clone(),
                parameters: record.parameters.clone(),
                properties: record.properties.clone(),
            }
        })
    }

    pub(super) fn encode_wrap(
        &self,
        profile: &ProfileRef,
        face: &FaceSelection,
        mode: &WrapMode,
        depth: &Option<Length>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            require_same_family(existing, &feature.id, &["Wrap"])?;
            let profile = profile_source(profile, record_sources, feature_sources, sketch_sources)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "SLDPRT feature {} references a missing wrap profile",
                        feature.id
                    ))
                })?;
            let face = face_selection_value(face).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT feature {} has no wrap target face",
                    feature.id
                ))
            })?;
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            match mode {
                WrapMode::Emboss | WrapMode::Deboss => {
                    let depth = depth
                        .filter(|value| value.0.is_finite() && value.0 > 0.0)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "SLDPRT feature {} has invalid wrap depth",
                                feature.id
                            ))
                        })?;
                    parameters.insert("Depth".into(), format_length_mm(depth.0));
                }
                WrapMode::Scribe => {
                    if depth.is_some() {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} gives a scribe wrap a depth",
                            feature.id
                        )));
                    }
                    parameters.remove("Depth");
                }
            }
            let mut properties = feature.source_properties.clone();
            properties.insert("Profile".into(), profile);
            properties.insert("Face".into(), face);
            properties.insert(
                "Mode".into(),
                match mode {
                    WrapMode::Emboss => "Emboss",
                    WrapMode::Deboss => "Deboss",
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

    pub(super) fn encode_sketch(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            require_same_family(existing, &feature.id, &["Sketch"])?;
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Sketch".into(), |record| record.kind.clone()),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties: feature.source_properties.clone(),
            }
        })
    }

    pub(super) fn encode_spatial_sketch(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            require_same_family(existing, &feature.id, &["Sketch"])?;
            NeutralFeatureEncoding {
                kind: "3DSketch".into(),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties: feature.source_properties.clone(),
            }
        })
    }
}
