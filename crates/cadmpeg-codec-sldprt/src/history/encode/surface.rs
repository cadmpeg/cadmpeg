// SPDX-License-Identifier: Apache-2.0
//! Trim, extend, ruled, offset, knit, filled, draft, thicken, and shell write encoders.

use super::super::{
    edge_selection_value, face_selection_value, feature_family, feature_input_class,
    format_angle_rad, format_length_like, format_length_mm, format_vector3, path_source,
    require_direction, require_same_family, write_native_selection,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::NativeClassKind;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, RuledSurfaceMode};

#[allow(
    clippy::unnecessary_wraps,
    reason = "Per-feature encoders use one fallible dispatch interface."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_boundary_surface_unresolved(
        &self,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::BoundarySurfaceUnresolved = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} has unresolved boundary-surface construction",
            feature.id
        )))
    }

    pub(super) fn encode_trim_surface(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::TrimSurface { faces, tool, keep } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            let faces = face_selection_value(faces).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} has no trim-surface input faces",
                    feature.id
                ))
            })?;
            let tool = path_source(tool, record_sources, sketch_sources).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} references a missing trim path",
                    feature.id
                ))
            })?;
            require_same_family(existing, &feature.id, &["TrimSurface", "SurfaceTrim"])?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Faces".into(), faces);
            properties.insert("Tool".into(), tool);
            properties.insert(
                "Keep".into(),
                crate::feature_schema::trim_region_token(*keep).into(),
            );
            (
                existing.map_or_else(|| "TrimSurface".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }

    pub(super) fn encode_extend_surface(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::ExtendSurface {
            faces,
            distance,
            method,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            let faces = face_selection_value(faces).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} has no extend-surface input faces",
                    feature.id
                ))
            })?;
            require_same_family(existing, &feature.id, &["ExtendSurface", "SurfaceExtend"])?;
            let distance = distance.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved surface extension distance",
                    feature.id
                ))
            })?;
            if !distance.0.is_finite() || distance.0 <= 0.0 {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has an invalid surface extension",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            parameters.insert("Distance".into(), format_length_mm(distance.0));
            let mut properties = feature.source_properties.clone();
            properties.insert("Faces".into(), faces);
            properties.insert(
                "Method".into(),
                crate::feature_schema::surface_extension_token(*method).into(),
            );
            (
                existing.map_or_else(|| "ExtendSurface".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_ruled_surface(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::RuledSurface {
            edges,
            support_faces,
            mode,
            angle,
            alternate_face,
            corner,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if angle.is_some() || alternate_face.is_some() || corner.is_some() {
                return Err(CodecError::Malformed(format!(
                                   "SLDPRT feature {} cannot encode ruled-surface angle, face-side, or corner semantics",
                                   feature.id
                               )));
            }
            let edges = edge_selection_value(edges).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} has no ruled-surface boundary edges",
                    feature.id
                ))
            })?;
            let support_faces = face_selection_value(support_faces).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} has no ruled-surface supports",
                    feature.id
                ))
            })?;
            require_same_family(existing, &feature.id, &["RuledSurface", "SurfaceRuled"])?;
            let (mode_name, direction, distance) = match mode {
                RuledSurfaceMode::Normal { distance } => ("Normal", None, *distance),
                RuledSurfaceMode::Tangent { distance } => ("Tangent", None, *distance),
                RuledSurfaceMode::Direction {
                    direction,
                    distance,
                } => {
                    require_direction(*direction, &feature.id, "ruled-surface direction")?;
                    ("Direction", Some(*direction), *distance)
                }
            };
            if !distance.0.is_finite() || distance.0 <= 0.0 {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has an invalid ruled-surface distance",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            parameters.insert("Distance".into(), format_length_mm(distance.0));
            let mut properties = feature.source_properties.clone();
            properties.insert("Edges".into(), edges);
            properties.insert("SupportFaces".into(), support_faces);
            properties.insert("Mode".into(), mode_name.into());
            match direction {
                Some(direction) => {
                    properties.insert("Direction".into(), format_vector3(direction));
                }
                None => {
                    properties.remove("Direction");
                }
            }
            (
                existing.map_or_else(|| "RuledSurface".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_shell(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Shell {
            bodies,
            removed_faces,
            thickness,
            outward,
            mode,
            join,
            resolve_intersections,
            allow_self_intersections,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if bodies.is_some()
                || mode.is_some()
                || join.is_some()
                || resolve_intersections.is_some()
                || allow_self_intersections.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported shell construction semantics",
                    feature.id
                )));
            }
            let selection = face_selection_value(removed_faces);
            if selection.is_none()
                && !(matches!(removed_faces, FaceSelection::Unresolved) && existing.is_some())
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported shell semantics",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !feature_family(record, "Shell")) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported shell semantics",
                    feature.id
                )));
            }
            if existing.is_none() && (thickness.is_none() || outward.is_none()) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved shell construction",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let thickness_key =
                if parameters.contains_key("D1") && !parameters.contains_key("Thickness") {
                    "D1"
                } else {
                    "Thickness"
                };
            if let Some(thickness) = thickness {
                parameters.insert(
                    thickness_key.into(),
                    format_length_like(
                        thickness.0,
                        existing
                            .and_then(|record| record.parameters.get(thickness_key))
                            .map(String::as_str),
                    ),
                );
            }
            let mut properties = feature.source_properties.clone();
            if let Some(selection) = selection {
                write_native_selection(
                    &mut properties,
                    "RemovedFaces",
                    &selection,
                    existing.map_or("", |record| record.id.as_str()),
                );
            }
            if let Some(outward) = outward {
                properties.insert("Outward".into(), outward.to_string());
            }
            (
                existing.map_or_else(|| "Shell".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_thicken(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Thicken {
            faces,
            thickness,
            side,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            use cadmpeg_ir::features::ThickenSide;

            let selection = face_selection_value(faces);
            if selection.is_none()
                && !(matches!(faces, FaceSelection::Unresolved) && existing.is_some())
                || existing.is_some_and(|record| {
                    !feature_family(record, "Thicken")
                        && !feature_family(record, "Thickness")
                        && !feature_input_class(record, NativeClassKind::Thicken)
                })
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported thicken semantics",
                    feature.id
                )));
            }
            if existing.is_none() && (thickness.is_none() || side.is_none()) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved thicken construction",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let thickness_key =
                if parameters.contains_key("D1") && !parameters.contains_key("Thickness") {
                    "D1"
                } else {
                    "Thickness"
                };
            if let Some(thickness) = thickness {
                parameters.insert(
                    thickness_key.into(),
                    format_length_like(
                        thickness.0,
                        existing
                            .and_then(|record| record.parameters.get(thickness_key))
                            .map(String::as_str),
                    ),
                );
            }
            let mut properties = feature.source_properties.clone();
            if let Some(selection) = selection {
                write_native_selection(
                    &mut properties,
                    "Faces",
                    &selection,
                    existing.map_or("", |record| record.id.as_str()),
                );
            }
            if let Some(side) = side {
                let both_sides = matches!(side, ThickenSide::Both);
                if both_sides || properties.contains_key("BothSides") {
                    properties.insert("BothSides".into(), both_sides.to_string());
                }
                let reverse = matches!(side, ThickenSide::Reverse);
                if reverse || properties.contains_key("Reverse") {
                    properties.insert("Reverse".into(), reverse.to_string());
                }
            }
            (
                existing.map_or_else(|| "Thicken".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_offset_surface(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::OffsetSurface { faces, distance } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            let selection = face_selection_value(faces).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} has no offset-surface support faces",
                    feature.id
                ))
            })?;
            require_same_family(existing, &feature.id, &["OffsetSurface"])?;
            let distance = distance.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved surface offset",
                    feature.id
                ))
            })?;
            if !distance.0.is_finite() {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has a non-finite surface offset",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            parameters.insert("Distance".into(), format_length_mm(distance.0));
            let mut properties = feature.source_properties.clone();
            properties.insert("Faces".into(), selection);
            (
                existing.map_or_else(|| "OffsetSurface".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_knit_surface(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::KnitSurface {
            faces,
            merge_entities,
            create_solid,
            gap_tolerance,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            let selection = face_selection_value(faces).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} has no knit-surface input faces",
                    feature.id
                ))
            })?;
            let merge_entities = merge_entities.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved knit merge state",
                    feature.id
                ))
            })?;
            let create_solid = create_solid.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved knit solid state",
                    feature.id
                ))
            })?;
            require_same_family(existing, &feature.id, &["KnitSurface", "Knit"])?;
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            match gap_tolerance {
                Some(value) if value.0.is_finite() && value.0 >= 0.0 => {
                    parameters.insert("GapTolerance".into(), format_length_mm(value.0));
                }
                Some(_) => {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an invalid knit tolerance",
                        feature.id
                    )));
                }
                None => {
                    parameters.remove("GapTolerance");
                }
            }
            let mut properties = feature.source_properties.clone();
            properties.insert("Faces".into(), selection);
            properties.insert("MergeEntities".into(), merge_entities.to_string());
            properties.insert("CreateSolid".into(), create_solid.to_string());
            (
                existing.map_or_else(|| "KnitSurface".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_filled_surface(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::FilledSurface {
            boundary,
            support_faces,
            continuity,
            boundary_continuities,
            merge_result,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            let cadmpeg_ir::features::SurfaceBoundary::Edges(boundary) = boundary else {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses a path-based filled-surface boundary",
                    feature.id
                )));
            };
            let boundary = edge_selection_value(boundary).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} has no filled-surface boundary",
                    feature.id
                ))
            })?;
            let support_faces = face_selection_value(support_faces).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} has no filled-surface supports",
                    feature.id
                ))
            })?;
            let continuity = continuity.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved filled-surface continuity",
                    feature.id
                ))
            })?;
            if !boundary_continuities.is_empty() {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has per-boundary filled-surface continuity",
                    feature.id
                )));
            }
            let merge_result = merge_result.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved filled-surface merge state",
                    feature.id
                ))
            })?;
            require_same_family(existing, &feature.id, &["FilledSurface", "FillSurface"])?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Boundary".into(), boundary);
            properties.insert("SupportFaces".into(), support_faces);
            properties.insert(
                "Continuity".into(),
                crate::feature_schema::surface_continuity_token(continuity).into(),
            );
            properties.insert("MergeResult".into(), merge_result.to_string());
            (
                existing.map_or_else(|| "FilledSurface".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }

    pub(super) fn encode_draft(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Draft {
            faces: face_selection,
            neutral_plane: plane_selection,
            parting_tool,
            pull_plane,
            pull_direction,
            angle,
            outward,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            let faces = face_selection_value(face_selection);
            let neutral_plane = face_selection_value(plane_selection);
            let operands_supported = |selection: &FaceSelection, native: Option<&String>| {
                native.is_some()
                    || matches!(selection, FaceSelection::Unresolved) && existing.is_some()
            };
            if existing.is_some_and(|record| !feature_family(record, "Draft"))
                || parting_tool.is_some()
                || pull_plane.is_some()
                || !operands_supported(face_selection, faces.as_ref())
                || !operands_supported(plane_selection, neutral_plane.as_ref())
                || existing.is_none()
                    && (pull_direction.is_none() || angle.is_none() || outward.is_none())
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported draft semantics",
                    feature.id
                )));
            }
            if let Some(pull_direction) = pull_direction {
                require_direction(*pull_direction, &feature.id, "draft direction")?;
            }
            if angle.is_some_and(|angle| !angle.0.is_finite()) {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has a non-finite draft angle",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            if let Some(angle) = angle {
                parameters.insert("Angle".into(), format_angle_rad(angle.0));
            }
            let mut properties = feature.source_properties.clone();
            let fallback = existing.map_or("", |record| record.id.as_str());
            if let Some(faces) = faces {
                write_native_selection(&mut properties, "Faces", &faces, fallback);
            }
            if let Some(neutral_plane) = neutral_plane {
                write_native_selection(&mut properties, "NeutralPlane", &neutral_plane, fallback);
            }
            if let Some(pull_direction) = pull_direction {
                properties.insert("Direction".into(), format_vector3(*pull_direction));
            }
            if let Some(outward) = outward {
                properties.insert("Outward".into(), outward.to_string());
            }
            (
                existing.map_or_else(|| "Draft".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }
}
