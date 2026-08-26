// SPDX-License-Identifier: Apache-2.0
//! Rib, pattern, and helical-sweep write encoders.

use super::super::{format_angle_rad, format_length_mm, pattern_form};
use super::format::{format_length_like, format_point3_mm, format_vector3};
use super::support::{
    path_source, profile_source, require_count, require_direction, require_same_family,
    resolved_boolean_op,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BooleanOp, PatternForm, PatternKind, PatternSeed, RibConstruction, RibDraft, RibSide,
};

#[allow(
    clippy::too_many_arguments,
    clippy::trivially_copy_pass_by_ref,
    clippy::ref_option,
    clippy::ptr_arg,
    reason = "Encoder arguments are borrowed from one FeatureDefinition match."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_rib(
        &self,
        construction: &RibConstruction,
        op: &BooleanOp,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            require_same_family(existing, &feature.id, &["Rib"])?;
            if existing.is_none()
                && (construction.profile.is_none()
                    || construction.direction.is_none()
                    || construction.thickness.is_none()
                    || construction.side.is_none()
                    || construction.draft == RibDraft::Unresolved
                    || *op == BooleanOp::Unresolved)
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved rib construction",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            if let Some(thickness) = construction.thickness {
                parameters.insert("Thickness".into(), format_length_mm(thickness.0));
            }
            match construction.draft {
                RibDraft::Angle(draft) => {
                    parameters.insert("Draft".into(), format_angle_rad(draft.0));
                }
                RibDraft::None => {
                    parameters.remove("Draft");
                }
                RibDraft::Unresolved => {}
            }
            let mut properties = feature.source_properties.clone();
            if let Some(profile) = &construction.profile {
                let profile_source =
                    profile_source(profile, record_sources, feature_sources, sketch_sources)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "SLDPRT feature {} references a missing rib profile",
                                feature.id
                            ))
                        })?;
                properties.insert("Profile".into(), profile_source);
            }
            if let Some(direction) = construction.direction {
                require_direction(direction, &feature.id, "rib direction")?;
                properties.insert("Direction".into(), format_vector3(direction));
            }
            if let Some(side) = construction.side {
                properties.insert("BothSides".into(), (side == RibSide::Centered).to_string());
            }
            if *op != BooleanOp::Unresolved {
                properties.insert(
                    "Operation".into(),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Rib".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_pattern(
        &self,
        seeds: &Vec<PatternSeed>,
        pattern: &PatternKind,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        let parent_sources = self.parent_sources;
        Ok({
            let expected_form = match pattern {
                PatternKind::Unresolved { form } => *form,
                PatternKind::Linear { .. } | PatternKind::LinearOffsets { .. } => {
                    Some(PatternForm::Linear)
                }
                PatternKind::Circular { .. } | PatternKind::CircularAngles { .. } => {
                    Some(PatternForm::Circular)
                }
                PatternKind::CurveDriven { .. } => Some(PatternForm::CurveDriven),
                PatternKind::Mirror { .. } | PatternKind::MirrorReference { .. } => {
                    Some(PatternForm::Mirror)
                }
                PatternKind::Scale { .. } => Some(PatternForm::Scale),
                PatternKind::Composite { .. } => Some(PatternForm::Composite),
            };
            if existing.is_some_and(|record| {
                expected_form.is_some_and(|form| pattern_form(record) != Some(form))
            }) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes pattern form",
                    feature.id
                )));
            }
            let mut seed_sources = Vec::new();
            for seed in seeds {
                match seed {
                    PatternSeed::Feature(seed) => {
                        seed_sources.push(parent_sources.get(seed).cloned().ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "SLDPRT feature {} references a missing pattern seed",
                                feature.id
                            ))
                        })?);
                    }
                    PatternSeed::Faces(_) | PatternSeed::Bodies(_) if existing.is_some() => {}
                    PatternSeed::Faces(_) | PatternSeed::Bodies(_) => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has a source-less topology pattern seed",
                            feature.id
                        )));
                    }
                    PatternSeed::Occurrences(_) => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has component-occurrence pattern seeds",
                            feature.id
                        )));
                    }
                }
            }
            if seed_sources.is_empty()
                && (!matches!(
                    expected_form,
                    Some(PatternForm::Linear | PatternForm::CurveDriven)
                ) || existing.is_none())
                && !matches!(pattern, PatternKind::Unresolved { .. })
            {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has no pattern seeds",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let mut properties = feature.source_properties.clone();
            if !seed_sources.is_empty() {
                properties.insert("Seeds".into(), seed_sources.join(","));
            }
            match pattern {
                PatternKind::Unresolved { .. } => {
                    if existing.is_none() {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has unresolved pattern construction",
                            feature.id
                        )));
                    }
                }
                PatternKind::Linear {
                    direction,
                    spacing,
                    count,
                    second,
                } => {
                    match direction {
                        Some(direction) => {
                            require_direction(*direction, &feature.id, "pattern")?;
                            properties.insert("Direction".into(), format_vector3(*direction));
                        }
                        None if existing.is_some() => {}
                        None => {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has an unresolved pattern direction",
                                feature.id
                            )));
                        }
                    }
                    require_count(*count, &feature.id)?;
                    if !spacing.0.is_finite() || spacing.0 <= 0.0 {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has invalid linear-pattern spacing",
                            feature.id
                        )));
                    }
                    let spacing_key =
                        if parameters.contains_key("D3") && !parameters.contains_key("Spacing") {
                            "D3"
                        } else {
                            "Spacing"
                        };
                    let count_key =
                        if parameters.contains_key("D1") && !parameters.contains_key("Count") {
                            "D1"
                        } else {
                            "Count"
                        };
                    parameters.insert(
                        spacing_key.into(),
                        format_length_like(
                            spacing.0,
                            existing
                                .and_then(|record| record.parameters.get(spacing_key))
                                .map(String::as_str),
                        ),
                    );
                    parameters.insert(count_key.into(), count.to_string());
                    if let Some(second) = second {
                        require_direction(second.direction, &feature.id, "second pattern")?;
                        require_count(second.count, &feature.id)?;
                        if !second.spacing.0.is_finite() || second.spacing.0 <= 0.0 {
                            return Err(CodecError::malformed(format_args!(
                                "SLDPRT feature {} has invalid second linear-pattern spacing",
                                feature.id
                            )));
                        }
                        properties.insert("Direction2".into(), format_vector3(second.direction));
                        parameters.insert("D4".into(), format_length_like(second.spacing.0, None));
                        parameters.insert("D2".into(), second.count.to_string());
                    }
                }
                PatternKind::Circular {
                    axis_origin,
                    axis_dir,
                    angle,
                    count,
                } => {
                    require_direction(*axis_dir, &feature.id, "pattern axis")?;
                    require_count(*count, &feature.id)?;
                    properties.insert("AxisOrigin".into(), format_point3_mm(*axis_origin));
                    properties.insert("AxisDirection".into(), format_vector3(*axis_dir));
                    parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    parameters.insert("Count".into(), count.to_string());
                }
                PatternKind::CurveDriven {
                    path,
                    spacing,
                    count,
                } => {
                    require_count(*count, &feature.id)?;
                    if !spacing.0.is_finite() || spacing.0 <= 0.0 {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has invalid curve-pattern spacing",
                            feature.id
                        )));
                    }
                    match path {
                        Some(path) => {
                            let path = path_source(path, record_sources, sketch_sources)
                                .ok_or_else(|| {
                                    CodecError::malformed(format_args!(
                                        "SLDPRT feature {} references a missing pattern path",
                                        feature.id
                                    ))
                                })?;
                            properties.insert("Path".into(), path);
                        }
                        None if existing.is_some() => {}
                        None => {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has an unresolved curve-pattern path",
                                feature.id
                            )));
                        }
                    }
                    let spacing_key =
                        if parameters.contains_key("D3") && !parameters.contains_key("Spacing") {
                            "D3"
                        } else {
                            "Spacing"
                        };
                    let count_key =
                        if parameters.contains_key("D1") && !parameters.contains_key("Count") {
                            "D1"
                        } else {
                            "Count"
                        };
                    parameters.insert(
                        spacing_key.into(),
                        format_length_like(
                            spacing.0,
                            existing
                                .and_then(|record| record.parameters.get(spacing_key))
                                .map(String::as_str),
                        ),
                    );
                    parameters.insert(count_key.into(), count.to_string());
                }
                PatternKind::Mirror {
                    plane_origin,
                    plane_normal,
                } => {
                    require_direction(*plane_normal, &feature.id, "mirror plane normal")?;
                    properties.insert("PlaneOrigin".into(), format_point3_mm(*plane_origin));
                    properties.insert("PlaneNormal".into(), format_vector3(*plane_normal));
                }
                PatternKind::MirrorReference { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved mirror plane",
                        feature.id
                    )));
                }
                PatternKind::LinearOffsets { .. }
                | PatternKind::CircularAngles { .. }
                | PatternKind::Scale { .. }
                | PatternKind::Composite { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses a pattern form that cannot be written",
                        feature.id
                    )));
                }
            }
            let kind = existing.map_or_else(
                || match expected_form {
                    Some(PatternForm::Linear) => "LinearPattern".into(),
                    Some(PatternForm::Circular) => "CircularPattern".into(),
                    Some(PatternForm::CurveDriven) => "CrvPattern".into(),
                    Some(PatternForm::Mirror) => "Mirror".into(),
                    Some(PatternForm::Scale | PatternForm::Composite) => "Pattern".into(),
                    None => "Pattern".into(),
                },
                |record| record.kind.clone(),
            );
            NeutralFeatureEncoding {
                kind,
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_helical_sweep(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses a helical sweep that cannot be written",
            feature.id
        )))
    }
}
