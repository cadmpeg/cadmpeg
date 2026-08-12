// SPDX-License-Identifier: Apache-2.0
//! Sketch, spatial-sketch, sketch-block, and wrap write encoders.

use super::super::{
    face_selection_value, feature_input_class, format_length_mm, profile_source,
    require_same_family, sketch_block_placement,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::NativeClassKind;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, WrapMode};

#[allow(
    clippy::unnecessary_wraps,
    reason = "Per-feature encoders use one fallible dispatch interface."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_sketch_block_definition(
        &self,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::SketchBlockDefinition { sketch } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
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
            (
                record.kind.clone(),
                record.parameters.clone(),
                record.properties.clone(),
            )
        })
    }

    pub(super) fn encode_sketch_block_instance(
        &self,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::SketchBlockInstance { block, placement } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
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
            (
                record.kind.clone(),
                record.parameters.clone(),
                record.properties.clone(),
            )
        })
    }

    pub(super) fn encode_wrap(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Wrap {
            profile,
            face,
            mode,
            depth,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            require_same_family(existing, &feature.id, &["Wrap"])?;
            let profile = profile_source(profile, record_sources, feature_sources, sketch_sources)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} references a missing wrap profile",
                        feature.id
                    ))
                })?;
            let face = face_selection_value(face).ok_or_else(|| {
                CodecError::Malformed(format!(
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
                            CodecError::Malformed(format!(
                                "SLDPRT feature {} has invalid wrap depth",
                                feature.id
                            ))
                        })?;
                    parameters.insert("Depth".into(), format_length_mm(depth.0));
                }
                WrapMode::Scribe => {
                    if depth.is_some() {
                        return Err(CodecError::Malformed(format!(
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
            (
                existing.map_or_else(|| "Wrap".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_sketch(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Sketch { .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            require_same_family(existing, &feature.id, &["Sketch"])?;
            (
                existing.map_or_else(|| "Sketch".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                feature.source_properties.clone(),
            )
        })
    }

    pub(super) fn encode_spatial_sketch(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::SpatialSketch { .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            require_same_family(existing, &feature.id, &["Sketch"])?;
            (
                "3DSketch".into(),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                feature.source_properties.clone(),
            )
        })
    }
}
