// SPDX-License-Identifier: Apache-2.0
//! Revolve, sweep, and loft write encoders.

use super::super::{format_angle_rad, valid_direction};
use super::format::{format_point3_mm, format_vector3};
use super::support::{
    is_loft, is_revolve, is_sweep, path_source, profile_source, resolved_boolean_op,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    Angle, AngularTermination, BooleanOp, LoftSection, PathRef, ProfileRef, RevolutionConstruction,
    RevolveExtent, SweepGuideRail, SweepMode, SweepOrientation, SweepPathExtent, SweepSection,
    SweepTransformation, SweepTransition,
};

#[allow(
    clippy::too_many_arguments,
    clippy::trivially_copy_pass_by_ref,
    clippy::ref_option,
    clippy::ptr_arg,
    reason = "Encoder arguments are borrowed from one FeatureDefinition match."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_revolve(
        &self,
        construction: &RevolutionConstruction,
        op: &BooleanOp,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            if construction.axis_reference.is_some()
                || construction.solid == Some(false)
                || construction.face_maker_class.is_some()
                || construction.fuse_order.is_some()
                || construction.allow_multi_profile_faces.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported revolution construction controls",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_revolve(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported revolution semantics",
                    feature.id
                )));
            }
            if existing.is_none()
                && (construction.profile.is_none()
                    || construction.axis.is_none()
                    || construction.extent.is_none()
                    || *op == BooleanOp::Unresolved)
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved revolution construction",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let mut properties = feature.source_properties.clone();
            if let Some(extent) = &construction.extent {
                parameters.remove("Angle");
                parameters.remove("Angle2");
                match extent {
                    RevolveExtent::OneSided {
                        termination: AngularTermination::Angle { angle },
                    } => {
                        properties.insert("EndCondition".into(), "OneSided".into());
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                    RevolveExtent::Symmetric {
                        termination: AngularTermination::Angle { angle },
                    } => {
                        properties.insert("EndCondition".into(), "Symmetric".into());
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                    RevolveExtent::TwoSided {
                        first: AngularTermination::Angle { angle: first },
                        second: AngularTermination::Angle { angle: second },
                    } => {
                        properties.insert("EndCondition".into(), "TwoSided".into());
                        parameters.insert("Angle".into(), format_angle_rad(first.0));
                        parameters.insert("Angle2".into(), format_angle_rad(second.0));
                    }
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses a linear revolution extent",
                            feature.id
                        )));
                    }
                }
            }
            if let Some(axis) = construction.axis {
                if !valid_direction(axis.direction) {
                    return Err(CodecError::malformed(format_args!(
                        "SLDPRT feature {} has a degenerate revolution axis",
                        feature.id
                    )));
                }
                properties.insert("AxisOrigin".into(), format_point3_mm(axis.origin));
                properties.insert("AxisDirection".into(), format_vector3(axis.direction));
            }
            if *op != BooleanOp::Unresolved {
                properties.insert(
                    "Operation".into(),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
            }
            if let Some(profile) = &construction.profile {
                let profile_source =
                    profile_source(profile, record_sources, feature_sources, sketch_sources)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "SLDPRT feature {} references a missing revolution profile",
                                feature.id
                            ))
                        })?;
                properties.insert("Profile".into(), profile_source);
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Revolve".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_sweep(
        &self,
        section: &SweepSection,
        sections: &Vec<SweepSection>,
        path: &Option<PathRef>,
        mode: &SweepMode,
        orientation: &Option<SweepOrientation>,
        transition: &Option<SweepTransition>,
        transformation: &Option<SweepTransformation>,
        path_tangent: &bool,
        linearize: &bool,
        twist: &Option<Angle>,
        path_extent: &Option<SweepPathExtent>,
        guide_rail: &Option<SweepGuideRail>,
        taper: &Option<Angle>,
        scale: &Option<f64>,
        allow_multi_profile_faces: &Option<bool>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            if !sections.is_empty()
                || orientation.is_some()
                || transition.is_some()
                || transformation.is_some()
                || *path_tangent
                || *linearize
                || path_extent.is_some()
                || guide_rail.is_some()
                || taper.is_some()
                || allow_multi_profile_faces.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported sweep construction semantics",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_sweep(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes operation family",
                    feature.id
                )));
            }
            let profile_source =
                match section {
                    cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Generated {
                        ..
                    }) if existing.is_some() => None,
                    cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Feature(_))
                        if existing
                            .is_some_and(|record| !record.properties.contains_key("Profile")) =>
                    {
                        None
                    }
                    cadmpeg_ir::features::SweepSection::Profile(profile) => Some(
                        profile_source(profile, record_sources, feature_sources, sketch_sources)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "SLDPRT feature {} references a missing sweep profile",
                                    feature.id
                                ))
                            })?,
                    ),
                    cadmpeg_ir::features::SweepSection::Unresolved(_) => None,
                    cadmpeg_ir::features::SweepSection::Generated(_) => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported generated sweep section",
                            feature.id
                        )));
                    }
                };
            let path_source = match path {
                Some(path) => Some(
                    path_source(path, record_sources, sketch_sources).ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "SLDPRT feature {} references a missing sweep path",
                            feature.id
                        ))
                    })?,
                ),
                None => None,
            };
            if existing.is_none() && (profile_source.is_none() || path_source.is_none()) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved sweep operands",
                    feature.id
                )));
            }
            if existing.is_none() && *mode == SweepMode::Unresolved {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved sweep result semantics",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            match twist {
                Some(twist) => {
                    parameters.insert("Twist".into(), format_angle_rad(twist.0));
                }
                None => {
                    parameters.remove("Twist");
                }
            }
            match scale {
                Some(scale) if scale.is_finite() && *scale > 0.0 => {
                    parameters.insert("Scale".into(), scale.to_string());
                }
                Some(_) => {
                    return Err(CodecError::malformed(format_args!(
                        "SLDPRT feature {} has an invalid sweep scale",
                        feature.id
                    )))
                }
                None => {
                    parameters.remove("Scale");
                }
            }
            let mut properties = feature.source_properties.clone();
            if let Some(profile) = profile_source {
                properties.insert("Profile".into(), profile);
            }
            if let Some(path) = path_source {
                properties.insert("Path".into(), path);
            }
            match mode {
                SweepMode::Solid { op } => {
                    properties.insert(
                        "Operation".into(),
                        resolved_boolean_op((*op).into(), &feature.id)?.into(),
                    );
                }
                SweepMode::NewBody => {
                    properties.insert("Operation".into(), "NewBody".into());
                }
                SweepMode::Surface => {
                    properties.remove("Operation");
                }
                SweepMode::Unresolved => {}
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(
                    || {
                        match mode {
                            SweepMode::Surface => "Surface-Sweep",
                            SweepMode::NewBody
                            | SweepMode::Solid { .. }
                            | SweepMode::Unresolved => "Sweep",
                        }
                        .into()
                    },
                    |record| record.kind.clone(),
                ),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_loft(
        &self,
        sections: &Vec<LoftSection>,
        guides: &Vec<PathRef>,
        centerline: &Option<PathRef>,
        op: &BooleanOp,
        closed: &bool,
        solid: &bool,
        ruled: &bool,
        linearize: &bool,
        max_degree: &Option<u32>,
        allow_multi_profile_faces: &Option<bool>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let feature_sources = self.feature_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            if centerline.is_some()
                || !solid
                || *ruled
                || *linearize
                || max_degree.is_some()
                || allow_multi_profile_faces.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported loft result semantics",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_loft(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported loft semantics",
                    feature.id
                )));
            }
            if existing.is_none() && (sections.len() < 2 || *op == BooleanOp::Unresolved) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved loft construction semantics",
                    feature.id
                )));
            }
            if sections
                .iter()
                .any(|section| matches!(section, cadmpeg_ir::features::LoftSection::Point(_)))
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses a point loft section",
                    feature.id
                )));
            }
            let profile_sources = sections
                .iter()
                .filter_map(|section| match section {
                    cadmpeg_ir::features::LoftSection::Profile(profile) => Some(profile),
                    cadmpeg_ir::features::LoftSection::Point(_) => None,
                })
                .map(|profile| {
                    profile_source(profile, record_sources, feature_sources, sketch_sources)
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "SLDPRT feature {} references a missing loft profile",
                        feature.id
                    ))
                })?;
            let guide_sources = guides
                .iter()
                .map(|path| path_source(path, record_sources, sketch_sources))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "SLDPRT feature {} references a missing loft guide",
                        feature.id
                    ))
                })?;
            let mut properties = feature.source_properties.clone();
            if !profile_sources.is_empty() || existing.is_none() {
                properties.insert("Profiles".into(), profile_sources.join(","));
            }
            if guide_sources.is_empty() && existing.is_none() {
                properties.remove("Guides");
            } else if !guide_sources.is_empty() {
                properties.insert("Guides".into(), guide_sources.join(","));
            }
            if *op != BooleanOp::Unresolved {
                properties.insert(
                    "Operation".into(),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
            }
            if *closed || existing.is_none() || properties.contains_key("Closed") {
                properties.insert("Closed".into(), closed.to_string());
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Loft".into(), |record| record.kind.clone()),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            }
        })
    }
}
