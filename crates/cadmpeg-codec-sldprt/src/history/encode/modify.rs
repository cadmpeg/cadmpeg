// SPDX-License-Identifier: Apache-2.0
//! Fillet, chamfer, combine, face/body edit, dome, flex, and scale write encoders.

use super::super::{
    body_retention_mode, format_angle_rad, format_length_mm, indexed_name, parse_bounded_angle_rad,
};
use super::format::{format_angle_like, format_length_like, format_point3_mm, format_vector3};
use super::support::{
    body_selection_value, edge_selection_value, face_selection_value, require_direction,
    require_same_family, resolved_boolean_op, write_native_selection,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::NativeClassKind;
use crate::history::classify::{feature_family, feature_input_class, is_chamfer, is_fillet};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    AxisAngle, BodyRetentionMode, BodySelection, BooleanOp, ChamferGroup, ChamferSpec,
    EdgeSelection, FaceMotion, FaceSelection, FilletGroup, FlexMode, Length, RadiusSpec,
    ScaleCenter, ScaleFactors,
};
use cadmpeg_ir::math::{Point3, Vector3};

#[allow(
    clippy::too_many_arguments,
    clippy::trivially_copy_pass_by_ref,
    clippy::ref_option,
    clippy::ptr_arg,
    reason = "Encoder arguments are borrowed from one FeatureDefinition match."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_fillet(
        &self,
        groups: &Vec<FilletGroup>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let [group] = groups.as_slice() else {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires exactly one fillet edge group",
                    feature.id
                )));
            };
            let edges = &group.edges;
            let radius = &group.radius;
            let selection = edge_selection_value(edges);
            if selection.is_none()
                && !(matches!(edges, EdgeSelection::Unresolved) && existing.is_some())
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported fillet semantics",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_fillet(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported fillet semantics",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let positional_radius = parameters.contains_key("D1")
                && !parameters.contains_key("Radius")
                && !parameters.keys().any(|name| indexed_name(name, "Radius"));
            match radius {
                RadiusSpec::Unresolved { .. } => {
                    if existing.is_none() {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has an unresolved fillet radius law",
                            feature.id
                        )));
                    }
                }
                RadiusSpec::Constant {
                    radius: Length(radius),
                } => {
                    parameters.retain(|name, _| {
                        name != "Radius"
                            && !indexed_name(name, "Radius")
                            && !indexed_name(name, "Position")
                    });
                    let key = if positional_radius { "D1" } else { "Radius" };
                    let value = format_length_like(
                        *radius,
                        existing
                            .and_then(|record| record.parameters.get(key))
                            .map(String::as_str),
                    );
                    parameters.insert(key.into(), value);
                }
                RadiusSpec::Chordal { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses a chordal fillet law",
                        feature.id
                    )));
                }
                RadiusSpec::Asymmetric { .. } => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses an asymmetric fillet law",
                        feature.id
                    )));
                }
                RadiusSpec::Variable { points } => {
                    parameters.retain(|name, _| {
                        name != "Radius"
                            && !indexed_name(name, "Radius")
                            && !indexed_name(name, "Position")
                    });
                    if positional_radius {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} changes positional fillet form",
                            feature.id
                        )));
                    }
                    if points.len() < 2
                        || points.iter().any(|point| {
                            !point.parameter.is_finite() || !(0.0..=1.0).contains(&point.parameter)
                        })
                        || points
                            .windows(2)
                            .any(|pair| pair[0].parameter >= pair[1].parameter)
                    {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has an invalid variable-radius law",
                            feature.id
                        )));
                    }
                    for (index, point) in points.iter().enumerate() {
                        parameters.insert(format!("Position{index}"), point.parameter.to_string());
                        parameters
                            .insert(format!("Radius{index}"), format_length_mm(point.radius.0));
                    }
                }
            }
            let mut properties = feature.source_properties.clone();
            if let Some(selection) = selection {
                write_native_selection(
                    &mut properties,
                    "Edges",
                    &selection,
                    existing.map_or("", |record| record.id.as_str()),
                );
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Fillet".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_chamfer(
        &self,
        groups: &Vec<ChamferGroup>,
        flip_direction: &bool,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let [group] = groups.as_slice() else {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires exactly one chamfer edge group",
                    feature.id
                )));
            };
            let edges = &group.edges;
            let spec = &group.spec;
            if *flip_direction {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses an unsupported reversed chamfer reference side",
                    feature.id
                )));
            }
            let selection = edge_selection_value(edges);
            if selection.is_none()
                && !(matches!(edges, EdgeSelection::Unresolved) && existing.is_some())
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported chamfer semantics",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_chamfer(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported chamfer semantics",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let positional = parameters.contains_key("D1")
                && !parameters.contains_key("Distance")
                && !parameters.contains_key("Distance1");
            let positional_angle = positional
                && parameters
                    .get("D2")
                    .is_some_and(|value| parse_bounded_angle_rad(value).is_some());
            match spec {
                ChamferSpec::Unresolved { .. } => {
                    if existing.is_none() {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has unresolved chamfer dimensions",
                            feature.id
                        )));
                    }
                }
                ChamferSpec::Distance { distance } => {
                    if existing.is_some()
                        && if positional {
                            parameters.contains_key("D2")
                        } else {
                            parameters.contains_key("Distance1")
                                || parameters.contains_key("Distance2")
                                || parameters.contains_key("Angle")
                        }
                    {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} changes chamfer form",
                            feature.id
                        )));
                    }
                    let key = if positional { "D1" } else { "Distance" };
                    let value = format_length_like(
                        distance.0,
                        existing
                            .and_then(|record| record.parameters.get(key))
                            .map(String::as_str),
                    );
                    parameters.insert(key.into(), value);
                }
                ChamferSpec::TwoDistances { first, second } => {
                    if existing.is_some()
                        && if positional {
                            !parameters.contains_key("D2") || positional_angle
                        } else {
                            !parameters.contains_key("Distance1")
                                || !parameters.contains_key("Distance2")
                                || parameters.contains_key("Angle")
                        }
                    {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} changes chamfer form",
                            feature.id
                        )));
                    }
                    let (first_key, second_key) = if positional {
                        ("D1", "D2")
                    } else {
                        ("Distance1", "Distance2")
                    };
                    parameters.insert(
                        first_key.into(),
                        format_length_like(
                            first.0,
                            existing
                                .and_then(|record| record.parameters.get(first_key))
                                .map(String::as_str),
                        ),
                    );
                    parameters.insert(
                        second_key.into(),
                        format_length_like(
                            second.0,
                            existing
                                .and_then(|record| record.parameters.get(second_key))
                                .map(String::as_str),
                        ),
                    );
                }
                ChamferSpec::DistanceAngle { distance, angle } => {
                    if existing.is_some()
                        && if positional {
                            !positional_angle
                        } else {
                            !parameters.contains_key("Distance")
                                || !parameters.contains_key("Angle")
                                || parameters.contains_key("Distance1")
                                || parameters.contains_key("Distance2")
                        }
                    {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} changes chamfer form",
                            feature.id
                        )));
                    }
                    let (distance_key, angle_key) = if positional {
                        ("D1", "D2")
                    } else {
                        ("Distance", "Angle")
                    };
                    parameters.insert(
                        distance_key.into(),
                        format_length_like(
                            distance.0,
                            existing
                                .and_then(|record| record.parameters.get(distance_key))
                                .map(String::as_str),
                        ),
                    );
                    parameters.insert(
                        angle_key.into(),
                        format_angle_like(
                            angle.0,
                            existing
                                .and_then(|record| record.parameters.get(angle_key))
                                .map(String::as_str),
                        ),
                    );
                }
            }
            let mut properties = feature.source_properties.clone();
            if let Some(selection) = selection {
                write_native_selection(
                    &mut properties,
                    "Edges",
                    &selection,
                    existing.map_or("", |record| record.id.as_str()),
                );
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Chamfer".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_combine(
        &self,
        target: &BodySelection,
        tools: &BodySelection,
        op: &BooleanOp,
        keep_tools: &bool,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            if existing.is_some_and(|record| {
                !feature_family(record, "Combine")
                    && !feature_input_class(record, NativeClassKind::Combine)
            }) || *op == BooleanOp::NewBody
                || *keep_tools
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported combine semantics",
                    feature.id
                )));
            }
            if existing.is_none()
                && (body_selection_value(target).is_none()
                    || body_selection_value(tools).is_none()
                    || *op == BooleanOp::Unresolved)
            {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has unresolved combine semantics",
                    feature.id
                )));
            }
            let mut properties = feature.source_properties.clone();
            if let Some(target) = body_selection_value(target) {
                properties.insert("Target".into(), target);
            }
            if let Some(tools) = body_selection_value(tools) {
                properties.insert("Tools".into(), tools);
            }
            if *op != BooleanOp::Unresolved {
                properties.insert(
                    "Operation".into(),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Combine".into(), |record| record.kind.clone()),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            }
        })
    }

    pub(super) fn encode_cut_with_surface(
        &self,
        targets: &BodySelection,
        tools: &FaceSelection,
        reverse: &Option<bool>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            require_same_family(existing, &feature.id, &["CutWithSurface", "SurfaceCut"])?;
            let targets = body_selection_value(targets).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT feature {} has no surface-cut target bodies",
                    feature.id
                ))
            })?;
            let tools = face_selection_value(tools).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT feature {} has no surface-cut tools",
                    feature.id
                ))
            })?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Targets".into(), targets);
            properties.insert("Tools".into(), tools);
            if let Some(reverse) = reverse {
                properties.insert("Reverse".into(), reverse.to_string());
            }
            NeutralFeatureEncoding {
                kind: existing
                    .map_or_else(|| "CutWithSurface".into(), |record| record.kind.clone()),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            }
        })
    }

    pub(super) fn encode_delete_body(
        &self,
        bodies: &BodySelection,
        mode: &BodyRetentionMode,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let selection = body_selection_value(bodies);
            if existing.is_some_and(|record| body_retention_mode(record).is_none()) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported delete-body semantics",
                    feature.id
                )));
            }
            let mut properties = feature.source_properties.clone();
            if let Some(selection) = selection {
                if !crate::resolved_features::component_paths::is_compact_body_selection_value(
                    &selection,
                ) {
                    properties.insert("Bodies".into(), selection);
                }
            } else if !matches!(mode, BodyRetentionMode::Unresolved) || existing.is_none() {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported delete-body semantics",
                    feature.id
                )));
            }
            match mode {
                BodyRetentionMode::Unresolved => {
                    let Some(record) = existing else {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} requires a retained unresolved body operation",
                            feature.id
                        )));
                    };
                    if body_retention_mode(record) != Some(BodyRetentionMode::Unresolved) {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} removes a resolved body-retention mode",
                            feature.id
                        )));
                    }
                    properties.remove("Mode");
                }
                BodyRetentionMode::DeleteSelected => {
                    properties.insert("Mode".into(), "Delete".into());
                }
                BodyRetentionMode::KeepSelected => {
                    properties.insert("Mode".into(), "Keep".into());
                }
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(
                    || match mode {
                        BodyRetentionMode::Unresolved => "Feature".into(),
                        BodyRetentionMode::DeleteSelected => "DeleteBody".into(),
                        BodyRetentionMode::KeepSelected => "KeepBody".into(),
                    },
                    |record| record.kind.clone(),
                ),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            }
        })
    }

    pub(super) fn encode_delete_face(
        &self,
        faces: &FaceSelection,
        heal: &bool,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let faces = face_selection_value(faces);
            if existing.is_some_and(|record| !feature_family(record, "DeleteFace"))
                || faces.is_none()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported delete-face semantics",
                    feature.id
                )));
            }
            let mut properties = feature.source_properties.clone();
            properties.insert("Faces".into(), faces.expect("checked above"));
            properties.insert("Heal".into(), heal.to_string());
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "DeleteFace".into(), |record| record.kind.clone()),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            }
        })
    }

    pub(super) fn encode_replace_face(
        &self,
        targets: &FaceSelection,
        replacements: &FaceSelection,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let targets = face_selection_value(targets);
            let replacements = face_selection_value(replacements);
            if existing.is_some_and(|record| !feature_family(record, "ReplaceFace"))
                || targets.is_none()
                || replacements.is_none()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported replace-face semantics",
                    feature.id
                )));
            }
            let mut properties = feature.source_properties.clone();
            properties.insert("Faces".into(), targets.expect("checked above"));
            properties.insert(
                "ReplacementFaces".into(),
                replacements.expect("checked above"),
            );
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "ReplaceFace".into(), |record| record.kind.clone()),
                parameters: existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            }
        })
    }

    pub(super) fn encode_move_face(
        &self,
        faces: &FaceSelection,
        motion: &FaceMotion,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let faces = face_selection_value(faces);
            if existing.is_some_and(|record| !feature_family(record, "MoveFace")) || faces.is_none()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported move-face semantics",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let mut properties = feature.source_properties.clone();
            properties.insert("Faces".into(), faces.expect("checked above"));
            parameters.remove("Distance");
            parameters.remove("Angle");
            properties.remove("Direction");
            properties.remove("AxisOrigin");
            properties.remove("AxisDirection");
            match motion {
                FaceMotion::Offset { distance } => {
                    properties.insert("Mode".into(), "Offset".into());
                    parameters.insert("Distance".into(), format_length_mm(distance.0));
                }
                FaceMotion::Translate {
                    direction,
                    distance,
                } => {
                    require_direction(*direction, &feature.id, "face translation")?;
                    properties.insert("Mode".into(), "Translate".into());
                    properties.insert("Direction".into(), format_vector3(*direction));
                    parameters.insert("Distance".into(), format_length_mm(distance.0));
                }
                FaceMotion::Rotate {
                    axis_origin,
                    axis_dir,
                    angle,
                } => {
                    require_direction(*axis_dir, &feature.id, "face rotation axis")?;
                    if !angle.0.is_finite() {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has a non-finite face rotation angle",
                            feature.id
                        )));
                    }
                    properties.insert("Mode".into(), "Rotate".into());
                    properties.insert("AxisOrigin".into(), format_point3_mm(*axis_origin));
                    properties.insert("AxisDirection".into(), format_vector3(*axis_dir));
                    parameters.insert("Angle".into(), format_angle_rad(angle.0));
                }
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "MoveFace".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_move_body(
        &self,
        bodies: &BodySelection,
        translation: &Vector3,
        rotation: &Option<AxisAngle>,
        copies: &u32,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let bodies = body_selection_value(bodies).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT feature {} has no body-motion selection",
                    feature.id
                ))
            })?;
            require_same_family(existing, &feature.id, &["MoveBody", "MoveCopyBody"])?;
            if ![translation.x, translation.y, translation.z]
                .into_iter()
                .all(f64::is_finite)
            {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has a non-finite body translation",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let mut properties = feature.source_properties.clone();
            properties.insert("Bodies".into(), bodies);
            properties.insert(
                "Translation".into(),
                format_point3_mm(Point3::new(translation.x, translation.y, translation.z)),
            );
            properties.insert("Copies".into(), copies.to_string());
            match rotation {
                Some(rotation) => {
                    require_direction(rotation.direction, &feature.id, "body rotation axis")?;
                    if !rotation.angle.0.is_finite()
                        || ![rotation.origin.x, rotation.origin.y, rotation.origin.z]
                            .into_iter()
                            .all(f64::is_finite)
                    {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has invalid body rotation",
                            feature.id
                        )));
                    }
                    properties.insert("RotationOrigin".into(), format_point3_mm(rotation.origin));
                    properties.insert("RotationAxis".into(), format_vector3(rotation.direction));
                    parameters.insert("Rotation".into(), format_angle_rad(rotation.angle.0));
                }
                None => {
                    properties.remove("RotationOrigin");
                    properties.remove("RotationAxis");
                    parameters.remove("Rotation");
                }
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "MoveBody".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_dome(
        &self,
        faces: &FaceSelection,
        height: &Option<Length>,
        elliptical: &Option<bool>,
        reverse: &Option<bool>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let faces = face_selection_value(faces);
            if existing.is_some_and(|record| !feature_family(record, "Dome")) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported dome semantics",
                    feature.id
                )));
            }
            if existing.is_none()
                && (faces.is_none()
                    || height.is_none()
                    || elliptical.is_none()
                    || reverse.is_none())
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved dome construction",
                    feature.id
                )));
            }
            if height.is_some_and(|height| !height.0.is_finite()) {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has a non-finite dome height",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            if let Some(height) = height {
                parameters.insert("Height".into(), format_length_mm(height.0));
            }
            let mut properties = feature.source_properties.clone();
            if let Some(faces) = faces {
                properties.insert("Faces".into(), faces);
            }
            if let Some(elliptical) = elliptical {
                properties.insert("Elliptical".into(), elliptical.to_string());
            }
            if let Some(reverse) = reverse {
                properties.insert("Reverse".into(), reverse.to_string());
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Dome".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_flex(
        &self,
        axis: &Option<Vector3>,
        mode: &FlexMode,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            if existing.is_some_and(|record| !feature_family(record, "Flex")) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported flex semantics",
                    feature.id
                )));
            }
            if existing.is_none() && (axis.is_none() || matches!(mode, FlexMode::Unresolved(_))) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved flex construction",
                    feature.id
                )));
            }
            if let Some(axis) = axis {
                require_direction(*axis, &feature.id, "flex axis")?;
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            let mut properties = feature.source_properties.clone();
            if let Some(axis) = axis {
                properties.insert("Axis".into(), format_vector3(*axis));
                properties.remove("AxisDirection");
            }
            match mode {
                FlexMode::Unresolved(_) => {}
                FlexMode::Bending { angle } => {
                    if !angle.0.is_finite() {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has a non-finite flex angle",
                            feature.id
                        )));
                    }
                    parameters.remove("Factor");
                    parameters.remove("Distance");
                    properties.insert("Mode".into(), "Bending".into());
                    parameters.insert("Angle".into(), format_angle_rad(angle.0));
                }
                FlexMode::Twisting { angle } => {
                    if !angle.0.is_finite() {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has a non-finite flex angle",
                            feature.id
                        )));
                    }
                    parameters.remove("Factor");
                    parameters.remove("Distance");
                    properties.insert("Mode".into(), "Twisting".into());
                    parameters.insert("Angle".into(), format_angle_rad(angle.0));
                }
                FlexMode::Tapering { factor } => {
                    if !factor.is_finite() || *factor <= 0.0 {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has an invalid flex taper factor",
                            feature.id
                        )));
                    }
                    parameters.remove("Angle");
                    parameters.remove("Distance");
                    properties.insert("Mode".into(), "Tapering".into());
                    parameters.insert("Factor".into(), factor.to_string());
                }
                FlexMode::Stretching { distance } => {
                    if !distance.0.is_finite() {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} has a non-finite flex distance",
                            feature.id
                        )));
                    }
                    parameters.remove("Angle");
                    parameters.remove("Factor");
                    properties.insert("Mode".into(), "Stretching".into());
                    parameters.insert("Distance".into(), format_length_mm(distance.0));
                }
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Flex".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_scale(
        &self,
        bodies: &BodySelection,
        center: &Option<ScaleCenter>,
        factors: &ScaleFactors,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let selection = body_selection_value(bodies);
            if existing.is_some_and(|record| !feature_family(record, "Scale")) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported scale semantics",
                    feature.id
                )));
            }
            let center_valid = center.as_ref().is_none_or(|center| match center {
                ScaleCenter::Point(point) => {
                    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
                }
                ScaleCenter::Native(reference) => !reference.is_empty(),
                ScaleCenter::Centroid | ScaleCenter::ModelOrigin => true,
            });
            let resolved_factors = factors.resolved();
            if existing.is_none()
                && (selection.is_none() || center.is_none() || resolved_factors.is_none())
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved scale construction",
                    feature.id
                )));
            }
            let factors_valid = resolved_factors.is_none_or(|factors| {
                [factors.x, factors.y, factors.z]
                    .into_iter()
                    .all(|factor| factor.is_finite() && factor != 0.0)
            });
            if !factors_valid || !center_valid {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has an invalid scale transform",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            match factors {
                ScaleFactors::Unresolved => {}
                ScaleFactors::Uniform(factor) => {
                    parameters.insert("Factor".into(), factor.to_string());
                    parameters.remove("ScaleX");
                    parameters.remove("ScaleY");
                    parameters.remove("ScaleZ");
                }
                ScaleFactors::PerAxis(factors) => {
                    parameters.remove("Factor");
                    parameters.insert("ScaleX".into(), factors.x.to_string());
                    parameters.insert("ScaleY".into(), factors.y.to_string());
                    parameters.insert("ScaleZ".into(), factors.z.to_string());
                }
            }
            let mut properties = feature.source_properties.clone();
            if let Some(selection) = selection {
                properties.insert("Bodies".into(), selection);
            }
            match center {
                Some(ScaleCenter::Centroid) => {
                    properties.remove("Center");
                    properties.remove("CenterRef");
                    properties.insert("CenterType".into(), "Centroid".into());
                }
                Some(ScaleCenter::ModelOrigin) => {
                    properties.remove("Center");
                    properties.remove("CenterRef");
                    properties.insert("CenterType".into(), "ModelOrigin".into());
                }
                Some(ScaleCenter::Point(point)) => {
                    properties.remove("CenterRef");
                    properties.insert("CenterType".into(), "Point".into());
                    properties.insert("Center".into(), format_point3_mm(*point));
                }
                Some(ScaleCenter::Native(reference)) => {
                    properties.remove("Center");
                    properties.insert("CenterType".into(), "Reference".into());
                    properties.insert("CenterRef".into(), reference.clone());
                }
                None => {}
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Scale".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }
}
