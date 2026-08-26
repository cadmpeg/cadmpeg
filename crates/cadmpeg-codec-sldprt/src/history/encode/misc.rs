// SPDX-License-Identifier: Apache-2.0
//! Tree-node, cosmetic-thread, native, curve, helix, and unsupported write encoders.

use super::super::{format_angle_rad, format_f64_literal, format_length_mm, valid_direction};
use super::format::{format_angle_like, format_length_like, format_point3_mm, format_vector3};
use super::support::{
    face_selection_value, feature_tree_node_kind, is_helix, path_source, require_direction,
    require_same_family,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::{classify, FeatureClass};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    Angle, CosmeticThreadExtent, CurveProjectionDirection, CurveProjectionDirectionState,
    FaceSelection, FeatureId, FeatureTreeNodeRole, HelixConstructionStyle, Length, PathRef,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;

#[allow(
    clippy::too_many_arguments,
    clippy::trivially_copy_pass_by_ref,
    clippy::ref_option,
    clippy::ptr_arg,
    reason = "Encoder arguments are borrowed from one FeatureDefinition match."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_tree_node(
        &self,
        role: &FeatureTreeNodeRole,
        children: &Vec<FeatureId>,
        active_child: &Option<FeatureId>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let retained_tree_node_roles = self.retained_tree_node_roles;
        Ok({
            if !children.is_empty() || active_child.is_some() {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses explicit tree membership",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| retained_tree_node_roles.get(&record.id) != Some(role))
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes feature-tree node role",
                    feature.id
                )));
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(
                    || feature_tree_node_kind(*role).into(),
                    |record| record.kind.clone(),
                ),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties: feature.source_properties.clone(),
            }
        })
    }

    pub(super) fn encode_cosmetic_thread(
        &self,
        face: &FaceSelection,
        diameter: &Option<Length>,
        extent: &Option<CosmeticThreadExtent>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let Some(record) =
                existing.filter(|record| classify(record) == Some(FeatureClass::CosmeticThread))
            else {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} adds a cosmetic thread",
                    feature.id
                )));
            };
            let mut parameters = record.parameters.clone();
            if let Some(diameter) = diameter {
                let prefix = record
                    .parameters
                    .get("D2")
                    .filter(|value| value.trim().starts_with("&lt;MOD-DIAM&gt;"))
                    .map_or("<MOD-DIAM>", |_| "&lt;MOD-DIAM&gt;");
                parameters.insert(
                    "D2".into(),
                    format!("{prefix}{}", format_f64_literal(diameter.0)),
                );
            }
            match extent {
                Some(CosmeticThreadExtent::Blind { length }) => {
                    parameters.insert(
                        "D1".into(),
                        format_length_like(
                            length.0,
                            record.parameters.get("D1").map(String::as_str),
                        ),
                    );
                }
                Some(CosmeticThreadExtent::Through) => {
                    parameters.remove("D1");
                }
                None => {}
            }
            let mut properties = feature.source_properties.clone();
            if let Some(value) = face_selection_value(face) {
                properties.insert("Face".into(), value);
            } else if !matches!(face, FaceSelection::Unresolved) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes cosmetic-thread face selection",
                    feature.id
                )));
            }
            NeutralFeatureEncoding {
                kind: record.kind.clone(),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_native(
        &self,
        kind: &String,
        parameters: &BTreeMap<String, String>,
        properties: &BTreeMap<String, String>,
    ) -> NeutralFeatureEncoding {
        let feature = self.feature;
        let mut merged = feature.source_properties.clone();
        merged.extend(properties.clone());
        NeutralFeatureEncoding {
            kind: kind.clone(),
            parameters: parameters.clone(),
            properties: merged,
        }
    }

    pub(super) fn encode_stored_geometry(&self) -> NeutralFeatureEncoding {
        let feature = self.feature;
        let existing = self.existing;
        NeutralFeatureEncoding {
            kind: existing.map_or_else(|| "Feature".into(), |record| record.kind.clone()),
            parameters: existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default(),
            properties: feature.source_properties.clone(),
        }
    }

    pub(super) fn encode_derived_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported copied-geometry semantics",
            feature.id
        )))
    }

    pub(super) fn encode_imported_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported external-import semantics",
            feature.id
        )))
    }

    pub(super) fn encode_primitive(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported analytic-primitive semantics",
            feature.id
        )))
    }

    pub(super) fn encode_equation_curve(
        &self,
        parameter: &String,
        x_expression: &String,
        y_expression: &String,
        z_expression: &String,
        start: &f64,
        end: &f64,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            if parameter.trim().is_empty()
                || x_expression.trim().is_empty()
                || y_expression.trim().is_empty()
                || z_expression.trim().is_empty()
                || !start.is_finite()
                || !end.is_finite()
                || start >= end
            {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has an invalid equation curve",
                    feature.id
                )));
            }
            require_same_family(
                existing,
                &feature.id,
                &["EquationDrivenCurve", "EquationCurve"],
            )?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Parameter".into(), parameter.clone());
            properties.insert("XEquation".into(), x_expression.clone());
            properties.insert("YEquation".into(), y_expression.clone());
            properties.insert("ZEquation".into(), z_expression.clone());
            properties.insert("Start".into(), start.to_string());
            properties.insert("End".into(), end.to_string());
            NeutralFeatureEncoding {
                kind: existing.map_or_else(
                    || "EquationDrivenCurve".into(),
                    |record| record.kind.clone(),
                ),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            }
        })
    }

    pub(super) fn encode_projected_curve(
        &self,
        source: &PathRef,
        target_faces: &FaceSelection,
        direction: &CurveProjectionDirection,
        bidirectional: &Option<bool>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            let source = path_source(source, record_sources, sketch_sources).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT feature {} references a missing projection source",
                    feature.id
                ))
            })?;
            let target_faces = face_selection_value(target_faces).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT feature {} has no projection target faces",
                    feature.id
                ))
            })?;
            require_same_family(
                existing,
                &feature.id,
                &["ProjectedCurve", "ProjectionCurve"],
            )?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Source".into(), source);
            properties.insert("TargetFaces".into(), target_faces);
            if let Some(bidirectional) = bidirectional {
                properties.insert("Bidirectional".into(), bidirectional.to_string());
            }
            match direction {
                CurveProjectionDirection::Vector(direction) => {
                    require_direction(*direction, &feature.id, "projection direction")?;
                    properties.insert("Direction".into(), format_vector3(*direction));
                }
                CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal) => {
                    properties.remove("Direction");
                }
                CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved) => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved projection direction",
                        feature.id
                    )));
                }
            }
            NeutralFeatureEncoding {
                kind: existing
                    .map_or_else(|| "ProjectedCurve".into(), |record| record.kind.clone()),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            }
        })
    }

    pub(super) fn encode_composite_curve(
        &self,
        segments: &Vec<PathRef>,
        closed: &bool,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            if segments.is_empty() {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has no composite-curve segments",
                    feature.id
                )));
            }
            require_same_family(existing, &feature.id, &["CompositeCurve"])?;
            let segments = segments
                .iter()
                .map(|segment| {
                    path_source(segment, record_sources, sketch_sources).ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "SLDPRT feature {} references a missing composite segment",
                            feature.id
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Segments".into(), segments.join(";"));
            properties.insert("Closed".into(), closed.to_string());
            NeutralFeatureEncoding {
                kind: existing
                    .map_or_else(|| "CompositeCurve".into(), |record| record.kind.clone()),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            }
        })
    }

    pub(super) fn encode_helix(
        &self,
        axis_origin: &Point3,
        axis_direction: &Vector3,
        radius: &Length,
        pitch: &Length,
        revolutions: &f64,
        start_angle: &Angle,
        clockwise: &bool,
        radial_growth: &Option<Length>,
        cone_angle: &Option<Angle>,
        segment_turns: &Option<f64>,
        construction_style: &Option<HelixConstructionStyle>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            if radial_growth.is_some()
                || cone_angle.is_some()
                || segment_turns.is_some()
                || construction_style.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported helix construction controls",
                    feature.id
                )));
            }
            if ![axis_origin.x, axis_origin.y, axis_origin.z, pitch.0]
                .into_iter()
                .all(f64::is_finite)
                || !valid_direction(*axis_direction)
                || !radius.0.is_finite()
                || radius.0 <= 0.0
                || !revolutions.is_finite()
                || *revolutions <= 0.0
                || !start_angle.0.is_finite()
            {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has invalid helix geometry",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_helix(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes operation family",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            parameters.insert("Radius".into(), format_length_mm(radius.0));
            parameters.insert("Pitch".into(), format_length_mm(pitch.0));
            parameters.insert("Revolutions".into(), revolutions.to_string());
            parameters.insert("StartAngle".into(), format_angle_rad(start_angle.0));
            let mut properties = feature.source_properties.clone();
            properties.insert("AxisOrigin".into(), format_point3_mm(*axis_origin));
            properties.insert("AxisDirection".into(), format_vector3(*axis_direction));
            properties.insert("Clockwise".into(), clockwise.to_string());
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Helix".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_helix_native_axis(
        &self,
        axis_native_ref: &String,
        axial_rise: &Length,
        pitch: &Length,
        revolutions: &f64,
        start_angle: &Angle,
        clockwise: &bool,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            if axis_native_ref.is_empty()
                || !axial_rise.0.is_finite()
                || !pitch.0.is_finite()
                || !revolutions.is_finite()
                || *revolutions <= 0.0
                || !start_angle.0.is_finite()
            {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has invalid native-axis helix geometry",
                    feature.id
                )));
            }
            let record = existing.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires a retained native helix axis",
                    feature.id
                ))
            })?;
            if !is_helix(record) || axis_native_ref != &record.id {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes its native helix axis",
                    feature.id
                )));
            }
            let mut parameters = record.parameters.clone();
            parameters.insert(
                "D3".into(),
                format_length_like(
                    axial_rise.0,
                    record.parameters.get("D3").map(String::as_str),
                ),
            );
            parameters.insert(
                "D4".into(),
                format_length_like(pitch.0, record.parameters.get("D4").map(String::as_str)),
            );
            parameters.insert("D5".into(), revolutions.to_string());
            parameters.insert(
                "D7".into(),
                format_angle_like(
                    start_angle.0,
                    record.parameters.get("D7").map(String::as_str),
                ),
            );
            let mut properties = feature.source_properties.clone();
            if properties.contains_key("Clockwise") || *clockwise {
                properties.insert("Clockwise".into(), clockwise.to_string());
            }
            NeutralFeatureEncoding {
                kind: record.kind.clone(),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_offset_shape(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported whole-shape offset semantics",
            feature.id
        )))
    }

    pub(super) fn encode_post_process(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported topology post-processing semantics",
            feature.id
        )))
    }

    pub(super) fn encode_curve_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported construction-geometry semantics",
            feature.id
        )))
    }

    pub(super) fn encode_shape_operation(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported derived-shape semantics",
            feature.id
        )))
    }

    pub(super) fn encode_binder(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses design-binder semantics that cannot be written",
            feature.id
        )))
    }

    pub(super) fn encode_explicitly_unsupported(
        &self,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses semantics that cannot be written",
            feature.id
        )))
    }

    pub(super) fn encode_unsupported(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses semantics that cannot be written",
            feature.id
        )))
    }
}
