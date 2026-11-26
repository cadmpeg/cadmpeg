// SPDX-License-Identifier: Apache-2.0
//! Rib, pattern, and helical-sweep write encoders.

use super::super::literals::{format_angle_rad, format_length_mm};
use super::super::project::pattern::{pattern_form, NativePatternClass};
use super::format::{format_length_like, format_point3_mm, format_vector3};
use super::support::{
    path_source, planar_profile_source, require_same_family, resolved_boolean_op,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::features::revolve::{RibConstruction, RibDraft, RibSide};

impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_rib(
        &self,
        construction: &RibConstruction,
        op: BooleanOp,
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
                    || op == BooleanOp::Unresolved)
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
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("Thickness"),
                    format_length_mm(thickness.into()),
                );
            }
            match construction.draft {
                RibDraft::Angle(draft) => {
                    parameters.insert(
                        cadmpeg_core::nonblank_literal!("Draft"),
                        format_angle_rad(draft.into()),
                    );
                }
                RibDraft::None => {
                    parameters.remove("Draft");
                }
                RibDraft::Unresolved => {}
            }
            let mut properties = feature.source_properties.clone();
            if let Some(profile) = &construction.profile {
                let profile_source =
                    planar_profile_source(profile, record_sources, feature_sources, sketch_sources)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "SLDPRT feature {} references a missing rib profile",
                                feature.id
                            ))
                        })?;
                properties.insert(cadmpeg_core::nonblank_literal!("Profile"), profile_source);
            }
            if let Some(direction) = construction.direction {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("Direction"),
                    format_vector3(direction.into()),
                );
            }
            if let Some(side) = construction.side {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("BothSides"),
                    (side == RibSide::Centered).to_string(),
                );
            }
            if op != BooleanOp::Unresolved {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("Operation"),
                    resolved_boolean_op(op, &feature.id)?.into(),
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
        seeds: &[PatternSeed],
        pattern: &PatternKind,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        let parent_sources = self.parent_sources;
        Ok({
            let unsupported_form = matches!(
                pattern.definition(),
                PatternTransform::Unresolved {
                    form: Some(
                        cadmpeg_ir::features::patterns::PatternForm::Scale
                            | cadmpeg_ir::features::patterns::PatternForm::Composite
                    )
                } | PatternTransform::Scale { .. }
                    | PatternTransform::Composite { .. }
            );
            let expected_form = match pattern.definition() {
                PatternTransform::Unresolved { form: None } => None,
                PatternTransform::Unresolved {
                    form: Some(cadmpeg_ir::features::patterns::PatternForm::Linear),
                } => Some(NativePatternClass::Linear),
                PatternTransform::Unresolved {
                    form: Some(cadmpeg_ir::features::patterns::PatternForm::Circular),
                } => Some(NativePatternClass::Circular),
                PatternTransform::Unresolved {
                    form: Some(cadmpeg_ir::features::patterns::PatternForm::CurveDriven),
                } => Some(NativePatternClass::CurveDriven),
                PatternTransform::Unresolved {
                    form: Some(cadmpeg_ir::features::patterns::PatternForm::Mirror),
                } => Some(NativePatternClass::Mirror),
                PatternTransform::Unresolved { .. }
                | PatternTransform::CurveDrivenPartial { .. } => None,
                PatternTransform::Linear { .. } | PatternTransform::LinearOffsets { .. } => {
                    Some(NativePatternClass::Linear)
                }
                PatternTransform::Circular { .. } | PatternTransform::CircularAngles { .. } => {
                    Some(NativePatternClass::Circular)
                }
                PatternTransform::CurveDriven { .. } => Some(NativePatternClass::CurveDriven),
                PatternTransform::Mirror { .. } | PatternTransform::MirrorReference { .. } => {
                    Some(NativePatternClass::Mirror)
                }
                PatternTransform::Scale { .. } | PatternTransform::Composite { .. } => None,
            };
            if existing.is_some_and(|record| {
                unsupported_form
                    || expected_form.is_some_and(|form| pattern_form(record) != Some(form))
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
                    PatternSeed::Faces(_) | PatternSeed::Bodies(_) | PatternSeed::Native(_) => {
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
            // A stored record states the seeds the decoder leaves unbound.
            if seed_sources.is_empty() && existing.is_none() && !pattern.is_unresolved() {
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
                properties.insert(
                    cadmpeg_core::nonblank_literal!("Seeds"),
                    seed_sources.join(","),
                );
            }
            match pattern.definition() {
                PatternTransform::Unresolved { .. } => {
                    if existing.is_none() {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has unresolved pattern construction",
                            feature.id
                        )));
                    }
                }
                PatternTransform::Linear {
                    direction,
                    spacing,
                    count,
                    second,
                    omitted_instances,
                    symmetric,
                } => {
                    if *symmetric || second.as_ref().is_some_and(|second| second.symmetric) {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has a symmetric pattern distribution",
                            feature.id
                        )));
                    }
                    if !omitted_instances.is_empty() {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has omitted pattern instances",
                            feature.id
                        )));
                    }
                    match direction {
                        Some(direction) => {
                            properties.insert(
                                cadmpeg_core::nonblank_literal!("Direction"),
                                format_vector3((*direction).into()),
                            );
                        }
                        None if existing.is_some() => {}
                        None => {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has an unresolved pattern direction",
                                feature.id
                            )));
                        }
                    }

                    let spacing_key =
                        if parameters.contains_key("D3") && !parameters.contains_key("Spacing") {
                            cadmpeg_core::nonblank_literal!("D3")
                        } else {
                            cadmpeg_core::nonblank_literal!("Spacing")
                        };
                    let count_key =
                        if parameters.contains_key("D1") && !parameters.contains_key("Count") {
                            cadmpeg_core::nonblank_literal!("D1")
                        } else {
                            cadmpeg_core::nonblank_literal!("Count")
                        };
                    parameters.insert(
                        spacing_key.clone(),
                        format_length_like(
                            (*spacing).into(),
                            existing
                                .and_then(|record| record.parameters.get(spacing_key.as_str()))
                                .map(String::as_str),
                        ),
                    );
                    parameters.insert(count_key.clone(), count.to_string());
                    if let Some(second) = second {
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("Direction2"),
                            format_vector3(second.direction.into()),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("D4"),
                            format_length_like(second.spacing.into(), None),
                        );
                        parameters.insert(
                            cadmpeg_core::nonblank_literal!("D2"),
                            second.count.to_string(),
                        );
                    }
                }
                PatternTransform::Circular {
                    axis_origin,
                    axis_dir,
                    angle,
                    count,
                } => {
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("AxisOrigin"),
                        format_point3_mm(*axis_origin),
                    );
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("AxisDirection"),
                        format_vector3((*axis_dir).into()),
                    );
                    parameters.insert(
                        cadmpeg_core::nonblank_literal!("Angle"),
                        format_angle_rad((*angle).into()),
                    );
                    parameters.insert(cadmpeg_core::nonblank_literal!("Count"), count.to_string());
                }
                PatternTransform::CurveDriven {
                    path,
                    spacing,
                    count,
                } => {
                    match path {
                        Some(path) => {
                            let path = path_source(path, record_sources, sketch_sources)
                                .ok_or_else(|| {
                                    CodecError::malformed(format_args!(
                                        "SLDPRT feature {} references a missing pattern path",
                                        feature.id
                                    ))
                                })?;
                            properties.insert(cadmpeg_core::nonblank_literal!("Path"), path);
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
                            cadmpeg_core::nonblank_literal!("D3")
                        } else {
                            cadmpeg_core::nonblank_literal!("Spacing")
                        };
                    let count_key =
                        if parameters.contains_key("D1") && !parameters.contains_key("Count") {
                            cadmpeg_core::nonblank_literal!("D1")
                        } else {
                            cadmpeg_core::nonblank_literal!("Count")
                        };
                    parameters.insert(
                        spacing_key.clone(),
                        format_length_like(
                            (*spacing).into(),
                            existing
                                .and_then(|record| record.parameters.get(spacing_key.as_str()))
                                .map(String::as_str),
                        ),
                    );
                    parameters.insert(count_key.clone(), count.to_string());
                }
                PatternTransform::Mirror {
                    plane_origin,
                    plane_normal,
                } => {
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("PlaneOrigin"),
                        format_point3_mm(*plane_origin),
                    );
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("PlaneNormal"),
                        format_vector3((*plane_normal).into()),
                    );
                }
                PatternTransform::MirrorReference { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved mirror plane",
                        feature.id
                    )));
                }
                PatternTransform::LinearOffsets { .. }
                | PatternTransform::CurveDrivenPartial { .. }
                | PatternTransform::CircularAngles { .. }
                | PatternTransform::Scale { .. }
                | PatternTransform::Composite { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses a pattern form that cannot be written",
                        feature.id
                    )));
                }
            }
            let kind = existing.map_or_else(
                || match expected_form {
                    Some(NativePatternClass::Linear) => "LinearPattern".into(),
                    Some(NativePatternClass::Circular) => "CircularPattern".into(),
                    Some(NativePatternClass::CurveDriven) => "CrvPattern".into(),
                    Some(NativePatternClass::Mirror) => "Mirror".into(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::serde_erase::json;
    use cadmpeg_ir::features::geometry::{FeatureDirection3, FinitePoint3};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::collections::HashMap;

    /// A mirror whose stored record states its seeds while the decoder binds
    /// none, and whose plane is resolved: the plane a lane binds.
    fn unbound_mirror() -> (crate::records::Feature, cadmpeg_ir::features::Feature) {
        let record: crate::records::Feature = json::from_value(serde_json::json!({
            "id":"native", "parent":"history", "ordinal":0,
            "name":"Reflect", "kind":"Mirror", "xml_tag":"Mirror",
            "input_class":"moMirrorPattern_c", "parameters":{},
            "properties":{"PlaneOrigin":"5mm,0mm,0mm", "PlaneNormal":"1,0,0"}
        }))
        .expect("synthetic feature record");
        let history: crate::records::FeatureHistory = json::from_value(serde_json::json!({
            "id":"history", "features":[record.clone()]
        }))
        .expect("synthetic history");
        let mut feature = crate::history::project::project_features(&[history])
            .expect("feature projection")
            .into_iter()
            .next()
            .expect("mirror");
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::new(PatternTransform::Mirror {
                    plane_origin: FinitePoint3::new(Point3::new(2.0, 0.0, 0.0))
                        .expect("finite origin"),
                    plane_normal: FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0))
                        .expect("nonzero normal"),
                })
                .expect("mirror pattern"),
            }));
        (record, feature)
    }

    fn encode(
        feature: &cadmpeg_ir::features::Feature,
        existing: Option<&crate::records::Feature>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        NeutralFeatureEncoder {
            contradicted_travel: false,
            feature,
            existing,
            principal_planes_by_record: &HashMap::new(),
            record_sources: &HashMap::new(),
            retained_tree_node_roles: &HashMap::new(),
            feature_sources: &HashMap::new(),
            sketch_sources: &HashMap::new(),
            parent_sources: &HashMap::new(),
            resolved_parameter_names: &HashMap::new(),
        }
        .encode()
    }

    /// A stored mirror record whose seeds the decoder leaves unbound encodes
    /// its edited plane, and its record keeps its seeds. A new mirror with no
    /// seeds has no record to state them.
    #[test]
    fn a_stored_mirror_with_unbound_seeds_encodes_its_plane() {
        let (record, feature) = unbound_mirror();

        let encoded = encode(&feature, Some(&record)).expect("the stored mirror encodes");
        assert_eq!(encoded.kind, "Mirror");
        assert_eq!(encoded.properties["PlaneOrigin"], "2mm,0mm,0mm");
        assert_eq!(encoded.properties["PlaneNormal"], "1,0,0");
        assert!(!encoded.properties.contains_key("Seeds"));

        let Err(error) = encode(&feature, None) else {
            panic!("a new mirror with no seeds encodes");
        };
        assert!(
            error.to_string().contains("has no pattern seeds"),
            "{error}"
        );
    }
}
